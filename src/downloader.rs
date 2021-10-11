use async_std::{future, prelude::*};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Mutex;
use std::sync::{Arc, RwLock};

pub struct DownloaderS {
    tasks_to_respond: Arc<Mutex<Vec<DownloaderInput>>>,
    responder: crossbeam_channel::Sender<Reply>,
    download_tasks: Vec<DownloadTask>,
}

impl DownloaderS {
    pub fn new(
        task_receiver: crossbeam_channel::Receiver<DownloaderInput>,
        responder: crossbeam_channel::Sender<Reply>,
    ) -> Self {
        let incoming_tasks = Arc::new(Mutex::new(vec![]));
        let thread_in_task = incoming_tasks.clone();
        std::thread::spawn(move || {
            while let Ok(data) = task_receiver.recv() {
                {
                    log::debug!("trying to lock tasks to add new");
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
            log::debug!("Check for download tasks");
            let mut dt = vec![];
            let mut futs = vec![];
            let mut complete_futs = vec![];
            let tasks: Vec<DownloaderInput> = {
                log::debug!("trying to get tasks");
                self.tasks_to_respond
                    .lock()
                    .expect("Cant lock tasks to get tasks")
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
            };
            log::debug!("Incoming tasks {}", tasks.len());
            for task in tasks.iter() {
                match &task {
                    DownloaderInput::DownloadTask(task) => {
                        let mut dtask = {
                            if let Some(task) = self
                                .download_tasks
                                .iter_mut()
                                .find(|p| p.video_id == task.video_id)
                            {
                                task.clone()
                            } else {
                                log::debug!("trying to create new download thread");
                                let dtask = DownloadTask::start_new_task(
                                    task.url.to_string(),
                                    task.video_id.clone(),
                                    task.file_path.clone(),
                                )
                                .await
                                .expect("Cant create download task");
                                self.download_tasks.push(dtask);
                                log::debug!("trying to get the just pushed thread");
                                self.download_tasks
                                    .last_mut()
                                    .expect("Just pushed!")
                                    .clone()
                            }
                        };
                        dt.push((dtask, task.clone()));
                    }
                    DownloaderInput::RemoveDownload(id) => {
                        log::info!("Removing download for video_id {}", id);
                        self.download_tasks.retain(|task| &task.video_id != id);
                        {
                            self.tasks_to_respond
                                .lock()
                                .expect("Cant lock tasks to get tasks")
                                .retain(|task| {
                                    !task
                                        .as_remove_download()
                                        .and_then(|taskid| Some(taskid == id))
                                        .unwrap_or(false)
                                })
                        }
                    }
                }
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
                            log::debug!("trying to remove task");
                            let pos = tasks
                                .iter()
                                .position(|t| {
                                    t.as_download_task()
                                        .and_then(|t| Some(t == &task))
                                        .unwrap_or(false)
                                })
                                .expect("Task not found to remove");
                            tasks.remove(pos);
                            log::debug!("trying to get download task to replace");
                            let dtask_pos =
                                match self.download_tasks.iter().position(|d| d.url == dtask.url) {
                                    Some(task) => task,
                                    None => {
                                        log::error!("Cant find task to replace old task");
                                        panic!("Cant find task to replace old task");
                                    }
                                };
                            self.download_tasks[dtask_pos] = dtask;
                        }
                        log::debug!("trying to reply download ");
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
                        log::debug!("trying to get download task");
                        let dtask_pos =
                            match self.download_tasks.iter().position(|d| d.url == dtask.url) {
                                Some(tak) => tak,
                                None => {
                                    log::error!("Downloading task not found");
                                    panic!("Downloading task not found")
                                }
                            };
                        log::debug!("Download task found, setting");
                        self.download_tasks[dtask_pos] = dtask;
                    }
                    Err(err) => {
                        log::warn!("Error when completing {:#?}", err);
                    }
                }
            }
            log::debug!("Sleep for next loop");
            async_std::task::sleep(std::time::Duration::from_millis(50)).await;
            log::debug!("Woke, continue next loop");
        }
    }
}
#[derive(Clone, PartialEq)]
pub enum DownloaderInput {
    DownloadTask(IncomingTask),
    RemoveDownload(String),
}

impl DownloaderInput {
    pub fn id(&self) -> String {
        match self {
            DownloaderInput::DownloadTask(task) => task.video_id.to_string(),
            DownloaderInput::RemoveDownload(id) => id.to_string(),
        }
    }

    pub fn as_download_task(&self) -> Option<&IncomingTask> {
        if let Self::DownloadTask(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_remove_download(&self) -> Option<&String> {
        if let Self::RemoveDownload(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct IncomingTask {
    pub url: String,
    pub pos: usize,
    pub buff: usize,
    pub video_id: String,
    pub file_path: Option<String>,
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
    pub file_name: Option<String>,
    pub video_id: String,
    pub has_cached: bool,
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
        log::info!("Creating new thread at position {}", pos);
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
            log::debug!("trying to get response to continue download");
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
    async fn start_new_task(
        url: String,
        video_id: String,
        file_name: Option<String>,
    ) -> Result<Self, anyhow::Error> {
        let client = surf::client();
        let length = {
            if let Ok(url) = surf::Url::parse(&url) {
                let response = client
                    .get(&url)
                    .await
                    .map_err(|e| anyhow::anyhow!("{:#?}", e));
                response
                    .and_then(|response| {
                        response
                            .len()
                            .ok_or(anyhow::anyhow!("Content length not known"))
                    })
                    .unwrap_or(0)
            } else {
                0
            }
        };
        log::info!("Content length found {}", length);

        let mut buff = vec![None; length];
        let mut has_cached = false;
        if let Some(path) = &file_name {
            match async_std::fs::read(path).await {
                Ok(data) => {
                    buff = data.iter().map(|item| Some(*item)).collect();
                    has_cached = true;
                }
                Err(err) => {
                    log::error!("Not cached!");
                }
            }
        }
        Ok(Self {
            url,
            len: length,

            has_cached,
            buff,
            client,
            download_progs: vec![],
            video_id,
            file_name,
        })
    }

    fn is_complete(&self) -> bool {
        self.buff.iter().all(|f| f.is_some())
    }

    async fn cache_to_file(&mut self) {
        if let Some(path) = &self.file_name {
            let content = self
                .buff
                .iter()
                .filter_map(|f| f.as_ref())
                .map(|f| *f)
                .collect::<Vec<_>>();
            let res = async_std::fs::write(path, content).await;
            if let Err(err) = res {
                log::error!("Cant save to file {:#?}", err);
            }
            self.has_cached = true;
        }
    }

    async fn complete_tasks(mut self) -> Result<Self, anyhow::Error> {
        // if self.is_complete() {
        // }

        if self.is_complete() {
            log::info!("Download complete");
            if !self.has_cached {
                self.cache_to_file().await;
            }
        }
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
                    log::debug!("Downloaded len {}", data.len());

                    return Ok(self);
                }
                Err(err) => {
                    log::warn!("Download failded {:#?}", err);
                    return Ok(self);
                }
            }
        } else {
            log::debug!("Nothing to download ");
            // log::info!("Download completed");

            return Ok(self);
        }
    }

    async fn download_task(
        mut self,
        task: IncomingTask,
    ) -> Result<(Vec<u8>, IncomingTask, Self), anyhow::Error> {
        let pos = task.pos;
        log::debug!("Requested data {:#?}", task);
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
            log::info!("returning cache len {}", return_vec.len());
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
                            if (!self.has_cached) {
                                self.cache_to_file().await;
                            }
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
