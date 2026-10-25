//! Vnode notifications deliver writes while a command keeps stdout open.
//! FSEvents can defer those changes until the writer closes the file.

use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

struct Selection {
    path: PathBuf,
    parent: File,
    file: Option<File>,
}

impl Selection {
    fn follow_file(&mut self, queue: &OwnedFd) -> io::Result<bool> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(self.file.take().is_some());
            }
            Err(error) => return Err(error),
        };
        if let Some(previous) = &self.file {
            let before = previous.metadata()?;
            let after = file.metadata()?;
            if (before.dev(), before.ino()) == (after.dev(), after.ino()) {
                return Ok(false);
            }
        }
        register(queue, &file)?;
        self.file = Some(file);
        Ok(true)
    }
}

pub struct OutputWatcher {
    queue: Arc<OwnedFd>,
    selection: Arc<Mutex<Option<Selection>>>,
    requested: Option<PathBuf>,
    worker: Option<JoinHandle<()>>,
}

impl OutputWatcher {
    pub fn new(
        mut changed: impl FnMut(notify::Result<notify::Event>) + Send + 'static,
    ) -> notify::Result<Self> {
        let fd = unsafe { libc::kqueue() };
        if fd == -1 {
            return Err(notify::Error::io(io::Error::last_os_error()));
        }
        let queue = Arc::new(unsafe { OwnedFd::from_raw_fd(fd) });
        update(
            &queue,
            0,
            libc::EVFILT_USER,
            libc::EV_ADD | libc::EV_CLEAR,
            0,
        )
        .map_err(notify::Error::io)?;
        let selection = Arc::new(Mutex::new(None::<Selection>));
        let selected = selection.clone();
        let events = queue.clone();
        let worker = std::thread::Builder::new()
            .name("output vnode watcher".into())
            .spawn(move || {
                loop {
                    let mut event = std::mem::MaybeUninit::<libc::kevent>::uninit();
                    let count = unsafe {
                        libc::kevent(
                            events.as_raw_fd(),
                            std::ptr::null(),
                            0,
                            event.as_mut_ptr(),
                            1,
                            std::ptr::null(),
                        )
                    };
                    if count == -1 {
                        let error = io::Error::last_os_error();
                        if error.kind() == io::ErrorKind::Interrupted {
                            continue;
                        }
                        changed(Err(notify::Error::io(error)));
                        break;
                    }
                    if count != 1 {
                        changed(Err(notify::Error::generic("Missing output vnode event")));
                        break;
                    }
                    let event = unsafe { event.assume_init() };
                    if event.flags & libc::EV_ERROR != 0 {
                        changed(Err(notify::Error::io(io::Error::from_raw_os_error(
                            event.data as i32,
                        ))));
                        continue;
                    }
                    if event.filter == libc::EVFILT_USER {
                        break;
                    }
                    let mut selected = selected.lock().expect("output watcher path poisoned");
                    let Some(selected) = selected.as_mut() else {
                        continue;
                    };
                    let ident = event.ident;
                    let file_changed = selected
                        .file
                        .as_ref()
                        .is_some_and(|file| file.as_raw_fd() as usize == ident);
                    let parent_changed = selected.parent.as_raw_fd() as usize == ident;
                    if !file_changed && !parent_changed {
                        continue;
                    }
                    match selected.follow_file(&events) {
                        Ok(replaced) if file_changed || replaced => {
                            changed(Ok(notify::Event::new(notify::EventKind::Modify(
                                notify::event::ModifyKind::Any,
                            ))
                            .add_path(selected.path.clone())))
                        }
                        Ok(_) => {}
                        Err(error) => changed(Err(notify::Error::io(error))),
                    }
                }
            })
            .map_err(notify::Error::io)?;
        Ok(Self {
            queue,
            selection,
            requested: None,
            worker: Some(worker),
        })
    }

    pub fn set_path(&mut self, path: Option<PathBuf>) -> notify::Result<bool> {
        if self.requested == path {
            return Ok(false);
        }
        self.requested = path.clone();
        let mut selected = self.selection.lock().expect("output watcher path poisoned");
        *selected = None;
        if let Some(path) = path {
            let parent = path
                .parent()
                .ok_or_else(|| notify::Error::generic("Output file has no parent directory"))?;
            let name = path
                .file_name()
                .ok_or_else(|| notify::Error::generic("Output file has no file name"))?;
            let parent = parent.canonicalize()?;
            let mut selection = Selection {
                path: parent.join(name),
                parent: File::open(parent)?,
                file: None,
            };
            register(&self.queue, &selection.parent).map_err(notify::Error::io)?;
            selection
                .follow_file(&self.queue)
                .map_err(notify::Error::io)?;
            *selected = Some(selection);
        }
        Ok(true)
    }
}

impl Drop for OutputWatcher {
    fn drop(&mut self) {
        // Wake the blocking kevent before joining; no timer or file polling is needed.
        update(&self.queue, 0, libc::EVFILT_USER, 0, libc::NOTE_TRIGGER)
            .expect("wake output watcher");
        self.worker
            .take()
            .expect("output watcher worker")
            .join()
            .expect("output watcher panicked");
    }
}

fn register(queue: &OwnedFd, file: &File) -> io::Result<()> {
    update(
        queue,
        file.as_raw_fd() as usize,
        libc::EVFILT_VNODE,
        libc::EV_ADD | libc::EV_CLEAR,
        libc::NOTE_WRITE
            | libc::NOTE_EXTEND
            | libc::NOTE_DELETE
            | libc::NOTE_RENAME
            | libc::NOTE_REVOKE,
    )
}

fn update(queue: &OwnedFd, ident: usize, filter: i16, flags: u16, fflags: u32) -> io::Result<()> {
    let event = libc::kevent {
        ident,
        filter,
        flags,
        fflags,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    if unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            &event,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    } == -1
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
