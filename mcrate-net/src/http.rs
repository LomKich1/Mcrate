//! Общие мелочи для работы с сетью: HTTP-клиент и перевод ошибок.
//! Функции `pub`: ими пользуются другие крейты (провайдеры, движок).

use std::time::Duration;

use mcrate_core::ProviderError;

// Притворяемся обычным браузером: так сайты отдают данные так же, как человеку.
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
(KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

/// Создаёт HTTP-клиент. Один клиент можно использовать для многих запросов (он держит соединения).
pub fn build_client() -> Result<reqwest::Client, ProviderError> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10)) // на установку соединения даём 10 секунд
        .read_timeout(Duration::from_secs(30)) // если данные не приходят 30 секунд подряд, сдаёмся
        // (общий таймаут на запрос не ставим: большой файл может честно качаться дольше)
        .build()
        .map_err(net_err)
}

/// Ошибка reqwest -> наша ошибка ProviderError::Network.
pub fn net_err(e: reqwest::Error) -> ProviderError {
    ProviderError::Network(e.to_string())
}

/// Первые `max` символов строки (по символам, а не по байтам: иначе можно разрезать кириллицу).
pub fn short(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}
