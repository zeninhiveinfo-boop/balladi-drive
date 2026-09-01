/// Prevent command-line child processes from creating a visible console window
/// in the packaged Windows GUI application.
#[cfg(target_os = "windows")]
pub fn hide_std_command_window(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
pub fn hide_std_command_window(_command: &mut std::process::Command) {}

#[cfg(target_os = "windows")]
pub fn hide_tokio_command_window(command: &mut tokio::process::Command) {
    hide_std_command_window(command.as_std_mut());
}

#[cfg(not(target_os = "windows"))]
pub fn hide_tokio_command_window(_command: &mut tokio::process::Command) {}
