use crossbeam_channel::Sender;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;

pub struct FolderWatcher {
    _watcher: RecommendedWatcher,
}

impl FolderWatcher {
    pub fn new<P: AsRef<Path>>(root: P, tx: Sender<()>) -> notify::Result<Self> {
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    if event.kind.is_modify() || event.kind.is_create() || event.kind.is_remove() {
                        let _ = tx.try_send(());
                    }
                }
            },
            Config::default(),
        )?;

        watcher.watch(root.as_ref(), RecursiveMode::Recursive)?;
        Ok(Self { _watcher: watcher })
    }
}
