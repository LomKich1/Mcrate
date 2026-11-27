//! Одна копия приложения: повторный запуск не открывает второе окно, а показывает уже работающее (в том числе
//! спрятанное в трей).
//!
//! Как устроено (только Windows): первая копия создаёт именованное событие (объект ядра, виден всем процессам
//! своего сеанса пользователя). Вторая копия видит, что событие уже есть, оставляет пути из командной строки во
//! временной папке («ящике»), сигналит событием и сразу выходит. Первая копия ждёт сигнал в отдельном потоке,
//! забирает пути и будит интерфейс; окно показывает `background.rs`. Событие живёт, пока жив процесс, поэтому после
//! выхода или падения следующий запуск снова будет первым.
//!
//! Не вышло создать событие: работаем как раньше, без этой защиты. Вне Windows защиты нет.

use std::fs;
use std::path::{Path, PathBuf};

/// Папка внутри временной папки пользователя, куда вторая копия кладёт пути для первой.
#[cfg_attr(not(windows), allow(dead_code))]
const INBOX_DIR: &str = "mcrate-open";

#[cfg_attr(not(windows), allow(dead_code))]
fn inbox() -> PathBuf {
    std::env::temp_dir().join(INBOX_DIR)
}

/// Кладёт пути в ящик, по одному на строку. Относительные делаем полными: рабочая папка у копий может различаться.
/// Сначала пишем во временный файл и переименовываем, чтобы первая копия не прочитала половину.
#[cfg_attr(not(windows), allow(dead_code))] // вне Windows пути никто не передаёт
fn leave_paths(dir: &Path, sender: u32, paths: &[PathBuf]) -> std::io::Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    let lines: Vec<String> =
        paths.iter().map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone()).to_string_lossy().into_owned()).collect();
    let draft = dir.join(format!("{sender}.tmp"));
    fs::write(&draft, lines.join("\n"))?;
    fs::rename(draft, dir.join(format!("{sender}.txt")))
}

/// Забирает все пути из ящика и удаляет прочитанные файлы.
#[cfg_attr(not(windows), allow(dead_code))]
fn take_paths(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut paths = Vec::new();
    for file in entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "txt")) {
        if let Ok(text) = fs::read_to_string(&file) {
            paths.extend(text.lines().filter(|l| !l.is_empty()).map(PathBuf::from));
        }
        let _ = fs::remove_file(file);
    }
    paths
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::path::PathBuf;
    use std::sync::mpsc::{self, Receiver};

    use super::{inbox, leave_paths, take_paths};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateEventW(attributes: *const c_void, manual_reset: i32, signaled: i32, name: *const u16) -> *mut c_void;
        fn SetEvent(event: *mut c_void) -> i32;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn AllowSetForegroundWindow(process: u32) -> i32;
    }

    /// Имя события. `Local\` = своё для каждого сеанса пользователя Windows.
    const EVENT_NAME: &str = r"Local\Mcrate.SingleInstance";
    const ERROR_ALREADY_EXISTS: i32 = 183;
    /// «Ждать без ограничения» и «дождались» для `WaitForSingleObject`.
    const INFINITE: u32 = u32::MAX;
    const WAIT_OBJECT_0: u32 = 0;
    /// «Любому процессу можно вывести своё окно вперёд».
    const ASFW_ANY: u32 = u32::MAX;

    /// Метка первой копии. Дескриптор события намеренно не закрываем: оно должно жить до конца процесса.
    pub(crate) struct SingleInstance {
        /// Дескриптор события числом (указатель нельзя передать в поток); 0 = защита не работает.
        event: usize,
    }

    /// `None`: приложение уже запущено, ему передан сигнал, этой копии пора выйти.
    pub(crate) fn claim(paths: &[PathBuf]) -> Option<SingleInstance> {
        let name: Vec<u16> = EVENT_NAME.encode_utf16().chain(Some(0)).collect();
        // SAFETY: `name` заканчивается нулём; событие с автосбросом (manual_reset = 0), сначала не сигнальное.
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
        let existed = std::io::Error::last_os_error().raw_os_error() == Some(ERROR_ALREADY_EXISTS); // читаем сразу
        if event.is_null() {
            return Some(SingleInstance { event: 0 });
        }
        if !existed {
            take_paths(&inbox()); // выбрасываем пути, оставшиеся от прошлого раза
            return Some(SingleInstance { event: event as usize });
        }
        let _ = leave_paths(&inbox(), std::process::id(), paths); // не записалось: хотя бы покажем окно
        // SAFETY: `event` действителен. Эту копию запустил пользователь, поэтому она вправе передать фокус первой.
        unsafe {
            AllowSetForegroundWindow(ASFW_ANY);
            SetEvent(event);
        }
        None
    }

    impl SingleInstance {
        /// Ждёт повторные запуски в отдельном потоке. Каждое сообщение: пути из командной строки второй копии
        /// (часто пусто). `ctx` будит приложение, даже если окно скрыто.
        pub(crate) fn listen(self, ctx: &egui::Context) -> Option<Receiver<Vec<PathBuf>>> {
            if self.event == 0 {
                return None;
            }
            let (tx, rx) = mpsc::channel();
            let ctx = ctx.clone();
            let wait = move || {
                // SAFETY: дескриптор получен от CreateEventW и не закрывается.
                while unsafe { WaitForSingleObject(self.event as *mut c_void, INFINITE) } == WAIT_OBJECT_0 {
                    if tx.send(take_paths(&inbox())).is_err() {
                        break; // приложение закрывается
                    }
                    ctx.request_repaint();
                }
            };
            std::thread::Builder::new().name("single-instance".into()).spawn(wait).ok()?;
            Some(rx)
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::PathBuf;
    use std::sync::mpsc::Receiver;

    /// Заглушка: вне Windows копий может быть сколько угодно.
    pub(crate) struct SingleInstance;

    pub(crate) fn claim(_paths: &[PathBuf]) -> Option<SingleInstance> {
        Some(SingleInstance)
    }

    impl SingleInstance {
        pub(crate) fn listen(self, _ctx: &egui::Context) -> Option<Receiver<Vec<PathBuf>>> {
            None
        }
    }
}

pub(crate) use imp::{SingleInstance, claim};

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mcrate-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn paths_pass_through_inbox_once() {
        let dir = test_dir("once");
        let song = std::env::temp_dir().join("Песня с пробелом.mp3");
        leave_paths(&dir, 1, std::slice::from_ref(&song)).unwrap();
        leave_paths(&dir, 2, std::slice::from_ref(&song)).unwrap();
        assert_eq!(take_paths(&dir), vec![song.clone(), song]);
        assert!(take_paths(&dir).is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn relative_paths_become_absolute() {
        let dir = test_dir("relative");
        leave_paths(&dir, 1, &[PathBuf::from("song.mp3")]).unwrap();
        assert!(take_paths(&dir)[0].is_absolute());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn no_paths_leave_nothing() {
        let dir = test_dir("empty");
        leave_paths(&dir, 1, &[]).unwrap();
        assert!(!dir.exists());
    }
}
