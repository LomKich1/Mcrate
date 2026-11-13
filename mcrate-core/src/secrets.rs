//! Хранилище токенов сервисов в Windows Credential Manager (системное
//! хранилище паролей Windows). Токены не попадают ни в код, ни в файлы проекта.

use crate::ProviderKind;  // `crate::` значит «из этого же крейта (mcrate-core)»
use keyring::Entry;       // Entry = одна запись в системном хранилище
use thiserror::Error;     // макрос для описания ошибок

#[derive(Debug, Error)]
pub enum SecretError {
    // #[from] автоматически превращает keyring::Error в нашу ошибку,
    // поэтому ниже можно писать `?` без ручной конвертации
    #[error("ошибка хранилища секретов: {0}")]
    Keyring(#[from] keyring::Error),
}

/// Читает и пишет токены. Внутри хранится только имя «папки» в хранилище.
pub struct TokenStore {
    service: String, // под этим именем записи видны в Credential Manager
}

impl TokenStore {
    /// Обычное хранилище приложения.
    pub fn new() -> Self {
        Self::with_service("mcrate")
    }

    /// Хранилище с другим именем. Нужно тестам, чтобы не трогать настоящие токены.
    pub fn with_service(service: &str) -> Self {
        Self { service: service.to_string() }
    }

    /// Имя записи для сервиса: у каждого сервиса свой токен.
    fn key_name(kind: ProviderKind) -> &'static str {
        match kind { // match = «выбери ветку по значению», как switch в C++
            ProviderKind::Yandex => "yandex-token",
            ProviderKind::Vk => "vk-token",
            ProviderKind::SoundCloud => "soundcloud-token",
            ProviderKind::Spotify => "spotify-token",
            ProviderKind::Local => "local-unused", // у своих файлов токена нет, имя-заглушка
        }
    }

    /// Создаёт объект Entry. `?` значит «если ошибка, сразу вернуть её наружу».
    fn entry(&self, kind: ProviderKind) -> Result<Entry, SecretError> {
        Ok(Entry::new(&self.service, Self::key_name(kind))?)
    }

    /// Сохранить токен (если уже был, перезапишется).
    pub fn save(&self, kind: ProviderKind, token: &str) -> Result<(), SecretError> {
        self.entry(kind)?.set_password(token)?;
        Ok(())
    }

    /// Получить токен. Option = «значение может быть или не быть»:
    /// Some(токен) если сохранён, None если ещё не подключали.
    pub fn load(&self, kind: ProviderKind) -> Result<Option<String>, SecretError> {
        match self.entry(kind)?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None), // записи нет: это не ошибка
            Err(e) => Err(e.into()),                  // настоящая ошибка
        }
    }

    /// Удалить токен («отключить сервис»). Если его и не было, тоже успех.
    pub fn delete(&self, kind: ProviderKind) -> Result<(), SecretError> {
        match self.entry(kind)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

// Default позволяет писать TokenStore::default() вместо TokenStore::new()
impl Default for TokenStore {
    fn default() -> Self {
        Self::new()
    }
}

// Блок ниже компилируется только при `cargo test`
#[cfg(test)]
mod tests {
    use super::*; // подтянуть всё из файла выше

    #[test] // помечает функцию как тест
    fn save_load_delete_roundtrip() {
        // Отдельное имя "mcrate-test", настоящие токены не затрагиваются
        let store = TokenStore::with_service("mcrate-test");
        let kind = ProviderKind::SoundCloud;

        store.delete(kind).unwrap();                 // чистим следы прошлых запусков
        assert_eq!(store.load(kind).unwrap(), None); // assert_eq! падает, если значения не равны

        store.save(kind, "fake-token-123").unwrap(); // unwrap() = «упади, если ошибка»
        assert_eq!(store.load(kind).unwrap(), Some("fake-token-123".to_string()));

        store.delete(kind).unwrap();
        assert_eq!(store.load(kind).unwrap(), None);
    }
}