// Windows 始终使用 GUI 子系统，避免开发版和发布版双击应用时显示控制台窗口。
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    desktop_lib::run();
}
