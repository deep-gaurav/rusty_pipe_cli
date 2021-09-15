use async_std::{future, prelude::*};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Mutex;
use std::sync::{Arc, RwLock};
pub struct DownloaderS {
    tasks_to_respond: Arc<Mutex<Vec<IncomingTask>>>,
    responder: std::sync::mpsc::Sender<Reply>,
    download_tasks: Vec<DownloadTask>,
}

impl DownloaderS {
    pub fn new(
        task_receiver: std::sync::mpsc::Receiver<IncomingTask>,
        responder: std::sync::mpsc::Sender<Reply>,
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
        loop {
            let tmpc = surf::client();
            let mut dt = vec![];
            let mut futs = vec![];
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
            .header("Range", format!("bytes={}", pos).as_str())
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
            response.read(&mut buff[..]).await
        };
        // log::info!("Downloaded data {:#?}", buff);
        if let Err(err) = resp {
            log::warn!("Download err {:#?}", err);
            return Err(anyhow::anyhow!("{:#?}", err));
        }
        if !buff.iter().any(|b| b != &0) {
            log::warn!("All 0s ");
            return Err(anyhow::anyhow!("All 0s downloaded"));
        }
        self.current_pos += buff.len();
        Ok(buff)
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
        let buff = vec![None; length];
        Ok(Self {
            url,
            len: length,
            buff,
            client,
            download_progs: vec![],
        })
    }

    async fn download_task(
        mut self,
        task: IncomingTask,
    ) -> Result<(Vec<u8>, IncomingTask, Self), anyhow::Error> {
        let pos = task.pos;
        if self.buff.len() < pos && self.buff[pos].is_some() {
            let mut return_vec = vec![];
            for i in self.buff[pos..].iter() {
                if let Some(i) = i {
                    return_vec.push(*i);
                } else {
                    break;
                }
            }
            log::info!("returning cache len {}", return_vec.len());
            return Ok((return_vec, task, self));
        }
        // todo!()
        loop {
            if let Some(down) = self
                .download_progs
                .iter_mut()
                .find(|p| p.current_pos == pos)
            {
                log::info!("Downloading using thread, pos -> {}", down.current_pos);
                let downloader = down.read(task.buff).await;
                log::info!(
                    "Downloadin complete using thread new pos {} ",
                    down.current_pos
                );
                match downloader {
                    Ok(data) => {
                        log::info!("Downloaded len {}", data.len());
                        for (i, data) in data.iter().enumerate() {
                            if self.buff.len() <= i + pos {
                                self.buff
                                    .append(&mut vec![None; (i + pos + 1) - self.buff.len()]);
                            }
                            self.buff[i + pos] = Some(*data);
                        }
                        return Ok((data, task, self));
                    }
                    Err(er) => {
                        log::warn!("{:#?}", er);
                    }
                }
            } else {
                log::info!("Create new download thread");
                let down_task =
                    DownloadProg::new_download_at(&self.url, pos, self.client.clone()).await;
                match down_task {
                    Ok(down_task) => {
                        log::info!("New thread created");
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
