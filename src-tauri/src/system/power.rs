use std::process::{Child, Command};
use std::sync::Mutex;

#[cfg(target_os = "windows")]
mod win {
    extern "system" {
        pub fn SetThreadExecutionState(es_flags: u32) -> u32;
    }
    pub const ES_CONTINUOUS: u32 = 0x80000000;
    pub const ES_SYSTEM_REQUIRED: u32 = 0x00000001;
    pub const ES_AWAYMODE_REQUIRED: u32 = 0x00000040;
}

struct PowerGuard {
    #[cfg(target_os = "macos")]
    child: Option<Child>,
    #[cfg(target_os = "windows")]
    win_guard: Option<(std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>)>,
}

static POWER_GUARD: Mutex<Option<PowerGuard>> = Mutex::new(None);

/// Prevent the operating system from going to sleep while transfers are in progress
pub fn acquire_sleep_lock() {
    let mut lock = POWER_GUARD.lock().unwrap();
    if lock.is_some() {
        return; // Idempotent: already active
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(child) = Command::new("caffeinate")
            .args(["-d", "-i", "-m", "-u"])
            .spawn()
        {
            *lock = Some(PowerGuard { child: Some(child) });
        }
    }

    #[cfg(target_os = "windows")]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let res = unsafe {
                win::SetThreadExecutionState(
                    win::ES_CONTINUOUS | win::ES_SYSTEM_REQUIRED | win::ES_AWAYMODE_REQUIRED,
                )
            };
            if res != 0 {
                let _ = rx.recv();
                unsafe {
                    win::SetThreadExecutionState(win::ES_CONTINUOUS);
                }
            }
        });
        *lock = Some(PowerGuard {
            win_guard: Some((tx, handle)),
        });
    }
}

/// Release the sleep prevention lock when transfers are paused or completed
pub fn release_sleep_lock() {
    let mut lock = POWER_GUARD.lock().unwrap();
    if let Some(guard) = lock.take() {
        #[cfg(target_os = "macos")]
        if let Some(mut child) = guard.child {
            let _ = child.kill();
            let _ = child.wait();
        }

        #[cfg(target_os = "windows")]
        if let Some((tx, handle)) = guard.win_guard {
            let _ = tx.send(());
            let _ = handle.join();
        }
    }
}
