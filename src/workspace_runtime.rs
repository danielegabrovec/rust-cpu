use crate::{
    bench::CpuBenchmark,
    model::{BenchmarkResult, Inventory, Snapshot},
    settings::{AlertEvent, Preferences},
    storage::{SavedBenchmark, SessionComparison, SessionData, SessionSummary, Store},
};
use anyhow::{Result, anyhow};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub enum Command {
    Start {
        name: String,
        inventory: Box<Inventory>,
        interval_ms: u64,
        first: Snapshot,
    },
    Append(Snapshot),
    Finish,
    Refresh,
    Load(i64),
    Compare(i64, i64),
    Rename(i64, String),
    Delete(i64),
    Preferences(Preferences),
    Cpu(CpuBenchmark),
    Llm(BenchmarkResult),
    Observed {
        value: BenchmarkResult,
        session: Option<i64>,
    },
    Alert(AlertEvent),
    Backup(PathBuf),
    ExportSession(i64, PathBuf),
    ExportComparison(i64, i64, PathBuf),
    ExportFrozenComparison(Box<SessionComparison>, PathBuf),
    Shutdown,
}
pub enum Reply {
    Started(i64),
    Finished,
    Refreshed {
        sessions: Vec<SessionSummary>,
        benchmarks: Vec<SavedBenchmark>,
        alerts: Vec<AlertEvent>,
    },
    Loaded(Box<SessionData>),
    Compared(Box<SessionComparison>),
    Saved(String),
    Error {
        message: String,
        recording_stopped: bool,
    },
}
pub struct ArchiveWorker {
    sender: mpsc::SyncSender<Command>,
    pub replies: mpsc::Receiver<Reply>,
    done: mpsc::Receiver<Result<(), String>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl ArchiveWorker {
    pub fn start(store: Store) -> Self {
        let (sender, rx) = mpsc::sync_channel::<Command>(128);
        let (tx, replies) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let thread = thread::spawn(move || {
            let mut store = store;
            let mut active = None;
            let mut shutdown_error = None;
            while let Ok(command) = rx.recv() {
                let stop = matches!(command, Command::Shutdown);
                let recording_command = matches!(
                    &command,
                    Command::Start { .. }
                        | Command::Append(_)
                        | Command::Finish
                        | Command::Shutdown
                );
                let previously_active = active;
                let result = (|| -> Result<()> {
                    match command {
                        Command::Start {
                            name,
                            inventory,
                            interval_ms,
                            first,
                        } => {
                            if active.is_some() {
                                return Err(anyhow!("Una sessione è già in registrazione"));
                            }
                            let id = store.begin_session_with_sample(
                                &name,
                                &inventory,
                                interval_ms,
                                &first,
                            )?;
                            active = Some(id);
                            let _ = tx.send(Reply::Started(id));
                        }
                        Command::Append(sample) => {
                            if let Some(id) = active
                                && let Err(e) = store.append_sample(id, &sample)
                            {
                                active = None;
                                return Err(e);
                            }
                        }
                        Command::Finish => {
                            if let Some(id) = active.take() {
                                store.finish_session(id)?;
                            }
                            let _ = tx.send(Reply::Finished);
                            refresh(&store, &tx)?;
                        }
                        Command::Refresh => refresh(&store, &tx)?,
                        Command::Load(id) => {
                            let _ = tx.send(Reply::Loaded(Box::new(store.load_session(id, 2000)?)));
                        }
                        Command::Compare(a, b) => {
                            let _ = tx.send(Reply::Compared(Box::new(store.compare(a, b)?)));
                        }
                        Command::Rename(id, name) => {
                            store.rename_session(id, &name)?;
                            refresh(&store, &tx)?;
                            let _ = tx.send(Reply::Saved("Nome sessione salvato".into()));
                        }
                        Command::Delete(id) => {
                            if active == Some(id) {
                                return Err(anyhow!(
                                    "Ferma la registrazione prima di eliminare la sessione attiva"
                                ));
                            }
                            store.delete_session(id)?;
                            refresh(&store, &tx)?;
                            let _ = tx.send(Reply::Saved("Sessione eliminata".into()));
                        }
                        Command::Preferences(p) => {
                            store.save_preferences(&p)?;
                        }
                        Command::Cpu(value) => {
                            store.save_cpu(active, &value)?;
                            refresh(&store, &tx)?;
                        }
                        Command::Llm(value) => {
                            store.save_llm(active, &value, false)?;
                            refresh(&store, &tx)?;
                        }
                        Command::Observed { mut value, session } => {
                            let session = match session {
                                Some(id)
                                    if matches!(store.session_state(id).as_deref(), Ok("open")) =>
                                {
                                    Some(id)
                                }
                                Some(_) => {
                                    if !value.note.is_empty() {
                                        value.note.push_str(" | ");
                                    }
                                    value.note.push_str("Associazione sessione scaduta/non disponibile: osservazione salvata separatamente.");
                                    None
                                }
                                None => None,
                            };
                            let id = store.save_llm(session, &value, true)?;
                            let _ =
                                tx.send(Reply::Saved(format!("Osservazione salvata SQLite #{id}")));
                        }
                        Command::Alert(value) => {
                            store.save_alert(active, &value)?;
                        }
                        Command::Backup(path) => {
                            store.backup(&path)?;
                            let _ =
                                tx.send(Reply::Saved(format!("Backup SQLite: {}", path.display())));
                        }
                        Command::ExportSession(id, path) => {
                            store.export_session(id, &path)?;
                            let _ =
                                tx.send(Reply::Saved(format!("Sessione JSON: {}", path.display())));
                        }
                        Command::ExportComparison(a, b, path) => {
                            let value = store.compare(a, b)?;
                            write_json_new(&path, &value)?;
                            let _ = tx
                                .send(Reply::Saved(format!("Confronto JSON: {}", path.display())));
                        }
                        Command::ExportFrozenComparison(value, path) => {
                            write_json_new(&path, &*value)?;
                            let _ = tx.send(Reply::Saved(format!(
                                "Confronto visualizzato JSON: {}",
                                path.display()
                            )));
                        }
                        Command::Shutdown => {
                            if let Some(id) = active.take() {
                                store.finish_session(id)?;
                            }
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    let mut message = format!("Archivio SQLite: {error:#}");
                    if recording_command
                        && let Some(id) = active.take().or(previously_active)
                        && let Err(error) = store.interrupt_session(id)
                    {
                        message.push_str(&format!("; stato incompleto non salvato: {error}"));
                    }
                    if stop {
                        shutdown_error = Some(message.clone());
                    }
                    let _ = tx.send(Reply::Error {
                        message,
                        recording_stopped: recording_command,
                    });
                }
                if stop {
                    break;
                }
            }
            // A disconnected sender also closes a healthy recording. Crashes retain committed rows.
            if let Some(id) = active.take()
                && let Err(error) = store.finish_session(id)
            {
                shutdown_error = Some(format!("Chiusura sessione SQLite non riuscita: {error}"));
            }
            drop(store);
            let _ = done_tx.send(shutdown_error.map_or(Ok(()), Err));
        });
        Self {
            sender,
            replies,
            done,
            thread: Some(thread),
        }
    }
    pub fn submit(&self, command: Command) -> Result<()> {
        self.sender.try_send(command).map_err(|e| match e {
            mpsc::TrySendError::Full(_) => {
                anyhow!("Coda SQLite piena: operazione non accettata, riprovare")
            }
            mpsc::TrySendError::Disconnected(_) => anyhow!("Worker SQLite non disponibile"),
        })
    }
    pub fn sender(&self) -> mpsc::SyncSender<Command> {
        self.sender.clone()
    }
    pub fn shutdown(&mut self) -> Result<()> {
        if self.thread.is_none() {
            return Ok(());
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        enqueue_until(&self.sender, Command::Shutdown, deadline)?;
        let completion = self.done.recv_timeout(deadline.saturating_duration_since(Instant::now())).map_err(|_|anyhow!("Salvataggio SQLite non confermato entro 5 s; controllare la sessione nell'archivio"))?;
        if let Some(handle) = self.thread.take() {
            handle
                .join()
                .map_err(|_| anyhow!("Worker SQLite interrotto"))?;
        }
        completion.map_err(|message| anyhow!(message))
    }
}
fn enqueue_until(
    sender: &mpsc::SyncSender<Command>,
    mut command: Command,
    deadline: Instant,
) -> Result<()> {
    loop {
        match sender.try_send(command) {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(anyhow!("Worker SQLite terminato prima del salvataggio"));
            }
            Err(mpsc::TrySendError::Full(value)) => command = value,
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(anyhow!(
                "Coda SQLite piena: chiusura non confermata entro il limite; i dati già committati restano nell'archivio"
            ));
        }
        thread::sleep(remaining.min(Duration::from_millis(2)));
    }
}

fn write_json_new(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let data = serde_json::to_vec_pretty(value)?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    if let Err(error) = file.write_all(&data).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error.into());
    }
    Ok(())
}
fn refresh(store: &Store, tx: &mpsc::Sender<Reply>) -> Result<()> {
    let _ = tx.send(Reply::Refreshed {
        sessions: store.list_sessions(500)?,
        benchmarks: store.benchmarks(None, 100)?,
        alerts: store.alerts(None, 100)?,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(cpu: f32) -> Snapshot {
        Snapshot {
            memory_total: 1000,
            memory_used: 100,
            cpu_percent: cpu,
            per_core: vec![cpu],
            ..Default::default()
        }
    }
    fn next_reply(worker: &ArchiveWorker) -> Reply {
        worker
            .replies
            .recv_timeout(Duration::from_secs(2))
            .expect("worker reply")
    }
    fn start_command(cpu: f32) -> Command {
        Command::Start {
            name: "test".into(),
            inventory: Box::default(),
            interval_ms: 1000,
            first: sample(cpu),
        }
    }

    #[test]
    fn full_queue_enqueue_is_bounded_before_the_done_wait() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        sender.try_send(Command::Refresh).ok().unwrap();
        let started = Instant::now();
        assert!(
            enqueue_until(
                &sender,
                Command::Shutdown,
                started + Duration::from_millis(20)
            )
            .is_err()
        );
        assert!(started.elapsed() < Duration::from_millis(500));
    }
    #[test]
    fn comparison_export_creates_parent_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/comparison.json");
        let value = serde_json::json!({"test": true});
        write_json_new(&path, &value).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).unwrap(),
            value
        );
        assert!(write_json_new(&path, &serde_json::json!({"replacement": true})).is_err());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).unwrap(),
            value
        );
    }
    #[test]
    fn failed_start_is_atomic_and_does_not_stick_recording_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.db");
        let mut worker = ArchiveWorker::start(Store::open(&path).unwrap());
        worker.submit(start_command(f32::NAN)).unwrap();
        assert!(matches!(
            next_reply(&worker),
            Reply::Error {
                recording_stopped: true,
                ..
            }
        ));
        worker.submit(start_command(10.0)).unwrap();
        assert!(matches!(next_reply(&worker), Reply::Started(_)));
        worker.shutdown().unwrap();
        let store = Store::open(&path).unwrap();
        let sessions = store.list_sessions(10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].state, "complete");
    }
    #[test]
    fn non_recording_error_preserves_recording_but_append_error_stops_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.db");
        let mut worker = ArchiveWorker::start(Store::open(&path).unwrap());
        worker.submit(start_command(10.0)).unwrap();
        let Reply::Started(id) = next_reply(&worker) else {
            panic!("session start");
        };
        worker.submit(Command::Load(999999)).unwrap();
        assert!(matches!(
            next_reply(&worker),
            Reply::Error {
                recording_stopped: false,
                ..
            }
        ));
        worker.submit(Command::Append(sample(20.0))).unwrap();
        worker.submit(Command::Append(sample(f32::NAN))).unwrap();
        assert!(matches!(
            next_reply(&worker),
            Reply::Error {
                recording_stopped: true,
                ..
            }
        ));
        worker.shutdown().unwrap();
        let data = Store::open(&path).unwrap().load_session(id, 20).unwrap();
        assert_eq!(data.total_samples, 2);
        assert_eq!(data.summary.state, "incomplete");
    }
    #[test]
    fn observed_association_is_rechecked_and_closed_session_falls_back_to_standalone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.db");
        let store = Store::open(&path).unwrap();
        let id = store
            .begin_session("external", &Inventory::default(), 1000)
            .unwrap();
        let mut worker = ArchiveWorker::start(Store::open(&path).unwrap());
        worker
            .submit(Command::Observed {
                value: BenchmarkResult::default(),
                session: Some(id),
            })
            .unwrap();
        assert!(matches!(next_reply(&worker), Reply::Saved(_)));
        store.finish_session(id).unwrap();
        worker
            .sender()
            .try_send(Command::Observed {
                value: BenchmarkResult::default(),
                session: Some(id),
            })
            .ok()
            .unwrap();
        assert!(matches!(next_reply(&worker), Reply::Saved(_)));
        worker.shutdown().unwrap();
        let values = store.benchmarks(None, 10).unwrap();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0].kind, "observed");
        assert_eq!(values[0].session_id, None);
        assert!(
            values[0]
                .llm
                .as_ref()
                .unwrap()
                .note
                .contains("Associazione sessione scaduta")
        );
        assert_eq!(values[1].session_id, Some(id));
    }
    #[test]
    fn frozen_comparison_export_matches_visible_data_after_new_live_samples() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.db");
        let mut store = Store::open(&path).unwrap();
        let left = store
            .begin_session_with_sample("left", &Inventory::default(), 1000, &sample(10.0))
            .unwrap();
        let right = store
            .begin_session_with_sample("right", &Inventory::default(), 1000, &sample(20.0))
            .unwrap();
        let frozen = store.compare(left, right).unwrap();
        assert_eq!(frozen.left.samples, 1);
        store.append_sample(left, &sample(90.0)).unwrap();
        assert_eq!(store.load_session(left, 20).unwrap().total_samples, 2);
        let exported = dir.path().join("frozen.json");
        let mut worker = ArchiveWorker::start(Store::open(&path).unwrap());
        worker
            .submit(Command::ExportFrozenComparison(
                Box::new(frozen),
                exported.clone(),
            ))
            .unwrap();
        assert!(matches!(next_reply(&worker), Reply::Saved(_)));
        worker.shutdown().unwrap();
        let value: SessionComparison =
            serde_json::from_slice(&std::fs::read(exported).unwrap()).unwrap();
        assert_eq!(value.left.samples, 1);
        assert_eq!(value.left_series.len(), 1);
        assert_eq!(value.left.cpu.average, Some(10.0));
        assert_eq!(value.left.state, "open");
    }
}
