use async_std::{future, prelude::*};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Mutex;
use std::sync::{Arc, RwLock};

pub struct DownloaderS {
    tasks_to_respond: Arc<Mutex<Vec<IncomingTask>>>,
    responder: crossbeam_channel::Sender<Reply>,
    download_tasks: Vec<DownloadTask>,
}

impl DownloaderS {
    pub fn new(
        task_receiver: crossbeam_channel::Receiver<IncomingTask>,
        responder: crossbeam_channel::Sender<Reply>,
    ) -> Self {
        let incoming_tasks = Arc::new(Mutex::new(vec![]));
        let thread_in_task = incoming_tasks.clone();
        std::thread::spawn(move || {
            while let Ok(data) = task_receiver.recv() {
                {
                    thread_in_task.lock().expect("Cant get tasks").push(data)
                }
            }
        });
        Self {
            tasks_to_respond: incoming_tasks,
            responder,
            download_tasks: vec![],
        }
    }

    pub async fn run(&mut self) {
        log::info!("Downloader started");

        loop {
            let mut dt = vec![];
            let mut futs = vec![];
            let mut complete_futs = vec![];
            let tasks: Vec<IncomingTask> = {
                self.tasks_to_respond
                    .lock()
                    .expect("Cant get tasks")
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
            };
            for task in tasks.iter() {
                let mut dtask = {
                    if let Some(task) = self.download_tasks.iter_mut().find(|p| p.url == task.url) {
                        task.clone()
                    } else {
                        let dtask = DownloadTask::start_new_task(task.url.to_string())
                            .await
                            .expect("Cant create download task");
                        self.download_tasks.push(dtask);
                        self.download_tasks
                            .last_mut()
                            .expect("Just pushed!")
                            .clone()
                    }
                };
                dt.push((dtask, task.clone()));
            }
            for (mut dtask, task) in dt {
                futs.push(dtask.download_task(task.clone()));
            }

            // Preload
            if futs.is_empty() {
                for task in self.download_tasks.iter() {
                    complete_futs.push(task.clone().complete_tasks())
                }
            }
            let results = futures::future::join_all(futs).await;
            for res in results {
                match res {
                    Ok((data, task, dtask)) => {
                        {
                            let mut tasks = self.tasks_to_respond.lock().expect("Cant loack");
                            let pos = tasks
                                .iter()
                                .position(|t| t == &task)
                                .expect("Task not found to remove");
                            tasks.remove(pos);
                            let dtask_pos = self
                                .download_tasks
                                .iter()
                                .position(|d| d.url == dtask.url)
                                .expect("download task not present");
                            self.download_tasks[dtask_pos] = dtask;
                        }
                        self.responder
                            .send(Reply { task, data })
                            .expect("Cant reply");
                    }
                    Err(err) => {
                        log::warn!("{:#?}", err);
                    }
                }
            }

            let complete_result = futures::future::join_all(complete_futs).await;
            for res in complete_result {
                match res {
                    Ok(dtask) => {
                        let dtask_pos = self
                            .download_tasks
                            .iter()
                            .position(|d| d.url == dtask.url)
                            .expect("download task not present");
                        self.download_tasks[dtask_pos] = dtask;
                    }
                    Err(err) => {
                        log::warn!("Error when completing {:#?}", err);
                    }
                }
            }
            async_std::task::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct IncomingTask {
    pub url: String,
    pub pos: usize,
    pub buff: usize,
}

pub struct Reply {
    pub task: IncomingTask,
    pub data: Vec<u8>,
}

#[derive(Clone)]
pub struct DownloadTask {
    pub url: String,
    pub len: usize,
    pub buff: Vec<Option<u8>>,
    pub client: surf::Client,
    pub download_progs: Vec<DownloadProg>,
}

#[derive(Clone)]
pub struct DownloadProg {
    current_pos: usize,
    response: Arc<Mutex<surf::Response>>,
}

impl DownloadProg {
    pub async fn new_download_at(
        url: &str,
        pos: usize,
        client: surf::Client,
    ) -> Result<Self, anyhow::Error> {
        let response = client
            .get(url)
            .header("Range", format!("bytes={}-", pos).as_str())
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("{:#?}", e))?;
        Ok(Self {
            current_pos: pos,
            response: Arc::new(Mutex::new(response)),
        })
    }
}

impl DownloadProg {
    pub async fn read(&mut self, size: usize) -> Result<Vec<u8>, anyhow::Error> {
        let mut buff = vec![0; size];
        let resp = {
            let mut response = self.response.lock().expect("Cant lock response");
            if response.is_empty().unwrap_or(true) {
                log::warn!("Body empty {:#?}", response.is_empty());
                Err(anyhow::anyhow!("Ended"))
            } else {
                response
                    .read(&mut buff[..])
                    .await
                    .map_err(|e| anyhow::anyhow!("{:#?}", e))
            }
        };
        // log::info!("Downloaded data {:#?}", buff);
        match resp {
            Ok(size) => {
                // log::info!("Downloaded {}", size);
                if size == 0 {
                    return Ok(vec![]);
                }

                if !buff.iter().any(|b| b != &0) {
                    log::warn!("All 0s ");
                    return Err(anyhow::anyhow!("All 0s downloaded"));
                }

                self.current_pos += size;
                Ok(buff[..size].iter().cloned().collect::<Vec<_>>())
            }
            Err(err) => {
                log::warn!("Download err {:#?}", err);
                // return Err(anyhow::anyhow!("{:#?}", err));
                return Ok(vec![]);
            }
        }
    }
}

impl DownloadTask {
    async fn start_new_task(url: String) -> Result<Self, anyhow::Error> {
        let client = surf::client();
        let response = client
            .get(&url)
            .await
            .map_err(|e| anyhow::anyhow!("{:#?}", e))?;
        let length = response
            .len()
            .ok_or(anyhow::anyhow!("Content length not known"))?;
        log::info!("Content length found {}", length);

        let buff = vec![None; length];
        Ok(Self {
            url,
            len: length,
            buff,
            client,
            download_progs: vec![],
        })
    }

    fn is_complete(&self) -> bool {
        self.buff.iter().all(|f| f.is_some())
    }

    async fn complete_tasks(mut self) -> Result<Self, anyhow::Error> {
        if let Some(prog_down) = self.download_progs.first_mut() {
            let pos = prog_down.current_pos;
            let result = prog_down.read(2048).await;
            match result {
                Ok(data) => {
                    // log::info!("Downloaded len {}", data.len());
                    let mut should_remove = false;
                    for (i, data) in data.iter().enumerate() {
                        if self.buff.len() <= i + pos {
                            log::warn!("Adding {}bytes", i + pos + 1);
                            self.buff
                                .append(&mut vec![None; (i + pos + 1) - self.buff.len()]);
                        }
                        if self.buff[i + pos].is_some() {
                            should_remove = true;
                        }
                        self.buff[i + pos] = Some(*data);
                    }
                    if data.len() == 0 {
                        should_remove = true;
                    }
                    if should_remove {
                        let task = self.download_progs.remove(0);
                        log::debug!("Removed download thread");
                    }

                    return Ok(self);
                }
                Err(err) => {
                    log::warn!("Download failded {:#?}", err);
                    return Ok(self);
                }
            }
        } else {
            // log::debug!("Nothing to download ");
            return Ok(self);
        }
    }

    async fn download_task(
        mut self,
        task: IncomingTask,
    ) -> Result<(Vec<u8>, IncomingTask, Self), anyhow::Error> {
        let pos = task.pos;
        if self.buff.len() > pos && self.buff[pos].is_some() {
            let mut return_vec = vec![];
            for i in self.buff[pos..].iter() {
                if return_vec.len() >= task.buff {
                    break;
                }
                if let Some(i) = i {
                    return_vec.push(*i);
                } else {
                    break;
                }
            }
            // log::info!("returning cache len {}", return_vec.len());
            return Ok((return_vec, task, self));
        }

        // log::info!(
        //     "All thread pos {:#?}",
        //     self.download_progs
        //         .iter()
        //         .map(|t| t.current_pos)
        //         .collect::<Vec<_>>()
        // );
        // todo!()
        loop {
            if let Some(down) = self
                .download_progs
                .iter_mut()
                .find(|p| p.current_pos == pos)
            {
                log::debug!(
                    "Found old thread reusing requested {} threadpos {}",
                    pos,
                    down.current_pos
                );
                // log::info!(
                //     "Downloading using thread, pos -> {} requesting size {}",
                //     down.current_pos,
                //     task.buff
                // );
                let downloader = down.read(task.buff).await;
                log::debug!(
                    "Downloadin complete using thread new pos {} ",
                    down.current_pos
                );
                match downloader {
                    Ok(data) => {
                        // log::info!("Downloaded len {}", data.len());
                        for (i, data) in data.iter().enumerate() {
                            if self.buff.len() <= i + pos {
                                log::warn!("Adding {}bytes", i + pos + 1);
                                self.buff
                                    .append(&mut vec![None; (i + pos + 1) - self.buff.len()]);
                            }
                            self.buff[i + pos] = Some(*data);
                        }

                        if self.is_complete() {
                            log::info!("Download complete");
                        }
                        return Ok((data, task, self));
                    }
                    Err(er) => {
                        log::warn!("{:#?}", er);
                    }
                }
            } else {
                log::debug!("Create new download thread");
                let down_task =
                    DownloadProg::new_download_at(&self.url, pos, self.client.clone()).await;
                match down_task {
                    Ok(down_task) => {
                        log::debug!("New thread created");
                        self.download_progs.push(down_task);
                    }
                    Err(err) => {
                        log::warn!("Cant create thread {:#?}", err);
                        return Err(err);
                    }
                }
            }
        }
    }
}
