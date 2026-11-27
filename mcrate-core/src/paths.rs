//! Где приложение хранит свои файлы на диске. Пути считает только ядро: экраны и контроллер их не собирают.
//!
//! Настройки, данные приложения и скачанные треки лежат внутри `%APPDATA%\Mcrate`.
//! Функции возвращают `Option`, потому что
//! переменной окружения теоретически может не быть: тогда приложение работает без сохранения.

use std::ffi::OsString;
use std::path::PathBuf;

/// Имя папки приложения внутри `%APPDATA%`.
const APP_DIR_NAME: &str = "Mcrate";

/// Папка приложения: `%APPDATA%\Mcrate`.
pub fn app_dir() -> Option<PathBuf> {
    app_dir_in(std::env::var_os("APPDATA"))
}

/// Папка постоянных загрузок: `%APPDATA%\Mcrate\downloads`.
pub fn downloads_dir() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("downloads"))
}

/// Кэш картинок обложек онлайн-треков: `%APPDATA%\Mcrate\covers`.
pub fn covers_dir() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("covers"))
}

/// Индекс метаданных скачанных треков: `%APPDATA%\Mcrate\downloads.txt`.
pub fn downloads_index_file() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("downloads.txt"))
}

/// Папка приложения внутри `base` (значение `APPDATA`); отдельной функцией, чтобы проверить без окружения.
fn app_dir_in(base: Option<OsString>) -> Option<PathBuf> {
    base.map(|dir| PathBuf::from(dir).join(APP_DIR_NAME))
}

/// Список сохранённых локальных треков: `library.txt`.
pub fn library_file() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("library.txt"))
}

/// История прослушивания: `history.txt`.
pub fn history_file() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("history.txt"))
}

/// Понравившиеся треки: `likes.txt`.
pub fn likes_file() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("likes.txt"))
}

/// Плейлисты: `playlists.txt`.
pub fn playlists_file() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("playlists.txt"))
}

/// Файл с публичным `client_id` SoundCloud: `soundcloud_client_id.txt`.
pub fn soundcloud_client_id_file() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("soundcloud_client_id.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_dir_is_mcrate_inside_appdata() {
        let dir = app_dir_in(Some(OsString::from("base"))).unwrap();
        assert_eq!(dir, PathBuf::from("base").join("Mcrate"));
    }

    #[test]
    fn no_appdata_means_no_dir() {
        assert_eq!(app_dir_in(None), None);
    }

    #[test]
    fn downloads_live_inside_appdata_mcrate() {
        assert_eq!(
            app_dir_in(Some(OsString::from("appdata"))).map(|dir| dir.join("downloads")),
            Some(PathBuf::from("appdata").join(APP_DIR_NAME).join("downloads")),
        );
    }

    #[test]
    fn downloads_index_lives_beside_downloads_folder() {
        assert_eq!(
            app_dir_in(Some(OsString::from("appdata"))).map(|dir| dir.join("downloads.txt")),
            Some(PathBuf::from("appdata").join(APP_DIR_NAME).join("downloads.txt")),
        );
    }
}
