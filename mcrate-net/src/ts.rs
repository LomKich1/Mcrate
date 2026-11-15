//! Достаёт звук из MPEG-TS (формат «куски по 188 байт», в нём HLS часто прячет MP3 и AAC).
//!
//! Зачем: плеер (rodio/symphonia) не умеет открывать MPEG-TS, зато умеет «голый» поток MP3
//! (кадры звука подряд). Поэтому мы разбираем TS сами и оставляем только звук.
//!
//! Как устроен TS (коротко):
//!   - Файл = куски по 188 байт («пакеты»), каждый начинается с байта 0x47 («синхробайт»).
//!   - В заголовке пакета есть PID (номер «дорожки»). PID 0 = таблица PAT: говорит,
//!     в каком PID лежит таблица PMT. PMT перечисляет дорожки и их типы (звук, видео...).
//!   - Сам звук приходит «посылками» PES: у каждой свой короткий заголовок, после него
//!     идут байты звука. Заголовки выбрасываем, байты склеиваем.

/// Размер одного пакета TS.
const PACKET: usize = 188;
const SYNC: u8 = 0x47;

/// Какой звук лежит в TS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsAudio {
    /// MPEG-аудио (MP3 или MP2), тип потока 0x03/0x04.
    Mpeg,
    /// AAC в обёртке ADTS, тип потока 0x0F.
    AacAdts,
}

/// Похоже ли это на MPEG-TS: синхробайт стоит в начале каждого из первых трёх пакетов.
pub fn looks_like_ts(data: &[u8]) -> bool {
    data.len() >= PACKET * 3 && data[0] == SYNC && data[PACKET] == SYNC && data[PACKET * 2] == SYNC
}

/// Вынимает звук из TS: возвращает «голый» поток (MP3-кадры или ADTS-кадры) и его тип.
pub fn extract_audio(data: &[u8]) -> Result<(Vec<u8>, TsAudio), String> {
    if !looks_like_ts(data) {
        return Err("это не MPEG-TS (нет синхробайтов 0x47 каждые 188 байт)".to_string());
    }

    let mut pmt_pid: Option<u16> = None; // где лежит таблица PMT (узнаём из PAT)
    let mut audio: Option<(u16, TsAudio)> = None; // PID дорожки со звуком и её тип (узнаём из PMT)
    let mut out: Vec<u8> = Vec::new();
    let mut in_pes = false; // идёт ли сейчас посылка звука (чтобы не брать «хвосты» чужих)

    for packet in data.chunks_exact(PACKET) {
        if packet[0] != SYNC {
            return Err("потерян синхробайт TS (повреждённые данные)".to_string());
        }
        let payload_start = packet[1] & 0x40 != 0; // «в этом пакете начинается новая посылка/таблица»
        let pid = (u16::from(packet[1] & 0x1F) << 8) | u16::from(packet[2]);
        let adaptation = (packet[3] >> 4) & 0b11; // бит 1 = есть доп. поле, бит 0 = есть полезные данные
        if adaptation & 0b01 == 0 {
            continue; // в пакете нет полезных данных
        }
        let mut pos = 4;
        if adaptation & 0b10 != 0 {
            pos += 1 + usize::from(packet[4]); // пропускаем дополнительное поле (его длина в байте 4)
        }
        if pos >= PACKET {
            continue;
        }
        let payload = &packet[pos..];

        if pid == 0 {
            // PAT: ищем номер PID, где лежит PMT
            if pmt_pid.is_none() && payload_start {
                pmt_pid = parse_pat(payload);
            }
        } else if Some(pid) == pmt_pid {
            // PMT: ищем дорожку со звуком
            if audio.is_none() && payload_start {
                audio = parse_pmt(payload)?;
            }
        } else if let Some((audio_pid, _)) = audio {
            if pid == audio_pid {
                if payload_start {
                    out.extend_from_slice(pes_payload(payload)?);
                    in_pes = true;
                } else if in_pes {
                    out.extend_from_slice(payload); // продолжение той же посылки
                }
            }
        }
    }

    match audio {
        Some((_, kind)) if !out.is_empty() => Ok((out, kind)),
        Some(_) => Err("в MPEG-TS есть звуковая дорожка, но без данных".to_string()),
        None => Err("в MPEG-TS не найдена звуковая дорожка (MP3 или AAC)".to_string()),
    }
}

/// Таблица PAT: возвращает PID таблицы PMT первой программы.
fn parse_pat(payload: &[u8]) -> Option<u16> {
    let section = section_body(payload)?;
    if section.first()? != &0x00 {
        return None; // это не PAT (table_id должен быть 0)
    }
    let length = (usize::from(section.get(1)? & 0x0F) << 8) | usize::from(*section.get(2)?);
    // Заголовок секции 8 байт (3 общих + 5), в конце 4 байта CRC; между ними записи по 4 байта
    let end = (3 + length).min(section.len()).checked_sub(4)?;
    let mut i = 8;
    while i + 4 <= end {
        let program = (u16::from(section[i]) << 8) | u16::from(section[i + 1]);
        let pid = (u16::from(section[i + 2] & 0x1F) << 8) | u16::from(section[i + 3]);
        if program != 0 {
            return Some(pid); // программа 0 = сетевая таблица, пропускаем
        }
        i += 4;
    }
    None
}

/// Таблица PMT: возвращает PID и тип первой звуковой дорожки, которую мы умеем.
fn parse_pmt(payload: &[u8]) -> Result<Option<(u16, TsAudio)>, String> {
    let Some(section) = section_body(payload) else { return Ok(None) };
    if section.first() != Some(&0x02) || section.len() < 12 {
        return Ok(None); // это не PMT
    }
    let length = (usize::from(section[1] & 0x0F) << 8) | usize::from(section[2]);
    let end = (3 + length).min(section.len()).saturating_sub(4); // без CRC
    let program_info = (usize::from(section[10] & 0x0F) << 8) | usize::from(section[11]);
    let mut i = 12 + program_info;
    let mut unsupported: Option<u8> = None;
    while i + 5 <= end {
        let stream_type = section[i];
        let pid = (u16::from(section[i + 1] & 0x1F) << 8) | u16::from(section[i + 2]);
        let es_info = (usize::from(section[i + 3] & 0x0F) << 8) | usize::from(section[i + 4]);
        match stream_type {
            0x03 | 0x04 => return Ok(Some((pid, TsAudio::Mpeg))),
            0x0F => return Ok(Some((pid, TsAudio::AacAdts))),
            0x11 => unsupported = Some(stream_type), // AAC в обёртке LATM: не поддержан
            _ => {}
        }
        i += 5 + es_info;
    }
    match unsupported {
        Some(t) => Err(format!("звук в MPEG-TS имеет тип 0x{t:02X}, такой не поддержан (умеем MP3 и AAC-ADTS)")),
        None => Ok(None),
    }
}

/// Пропускает «указатель» перед таблицей (первый байт полезных данных) и отдаёт саму секцию.
fn section_body(payload: &[u8]) -> Option<&[u8]> {
    let pointer = usize::from(*payload.first()?);
    payload.get(1 + pointer..)
}

/// Снимает заголовок PES и отдаёт байты звука.
fn pes_payload(payload: &[u8]) -> Result<&[u8], String> {
    // Заголовок: 00 00 01, id потока, длина (2), флаги (2), длина доп. полей (1), сами доп. поля
    if payload.len() < 9 || payload[0] != 0 || payload[1] != 0 || payload[2] != 1 {
        return Err("в MPEG-TS повреждена посылка звука (нет стартового кода 00 00 01)".to_string());
    }
    let start = 9 + usize::from(payload[8]);
    payload.get(start..).ok_or_else(|| "в MPEG-TS обрезан заголовок посылки звука".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Собирает один пакет TS: заголовок + (необязательное) заполнение + данные в конце пакета.
    fn packet(pid: u16, payload_start: bool, data: &[u8]) -> Vec<u8> {
        assert!(data.len() <= PACKET - 4);
        let mut p = vec![SYNC, (u8::from(payload_start) << 6) | ((pid >> 8) as u8 & 0x1F), pid as u8];
        let pad = PACKET - 4 - data.len();
        if pad == 0 {
            p.push(0x10); // только полезные данные
        } else {
            p.push(0x30); // есть дополнительное поле + данные
            p.push((pad - 1) as u8); // длина доп. поля
            if pad > 1 {
                p.push(0x00); // флаги
                p.extend(std::iter::repeat(0xFF).take(pad - 2)); // заполнитель
            }
        }
        p.extend_from_slice(data);
        assert_eq!(p.len(), PACKET);
        p
    }

    fn pat(pmt_pid: u16) -> Vec<u8> {
        // pointer 0, table_id 0, длина секции 13, ... программа 1 -> PMT в pmt_pid, CRC (любой: не проверяем)
        let mut s = vec![0x00, 0x00, 0xB0, 0x0D, 0x00, 0x01, 0xC1, 0x00, 0x00, 0x00, 0x01, 0xE0 | (pmt_pid >> 8) as u8, pmt_pid as u8];
        s.extend_from_slice(&[0, 0, 0, 0]);
        s
    }

    fn pmt(stream_type: u8, audio_pid: u16) -> Vec<u8> {
        // pointer 0, table_id 2, длина секции 18: 9 общих + одна дорожка (5) + CRC (4)
        let mut s = vec![0x00, 0x02, 0xB0, 0x12, 0x00, 0x01, 0xC1, 0x00, 0x00, 0xE1, 0x00, 0xF0, 0x00];
        s.extend_from_slice(&[stream_type, 0xE0 | (audio_pid >> 8) as u8, audio_pid as u8, 0xF0, 0x00]);
        s.extend_from_slice(&[0, 0, 0, 0]);
        s
    }

    /// Заголовок PES с `extra` байтами доп. полей (например, метка времени) и звуком `audio`.
    fn pes(audio: &[u8], extra: usize) -> Vec<u8> {
        let mut v = vec![0, 0, 1, 0xC0, 0, 0, 0x80, 0x80, extra as u8];
        v.extend(std::iter::repeat(0xAA).take(extra));
        v.extend_from_slice(audio);
        v
    }

    fn sample_ts(stream_type: u8) -> (Vec<u8>, Vec<u8>) {
        let audio: Vec<u8> = (0..400u32).map(|i| (i % 251) as u8).collect();
        let mut ts = Vec::new();
        ts.extend(packet(0, true, &pat(0x100)));
        ts.extend(packet(0x100, true, &pmt(stream_type, 0x101)));
        // Посторонняя дорожка (видео) и метаданные не должны попасть в результат
        ts.extend(packet(0x102, true, &pes(b"VIDEO-VIDEO", 0)));
        // Звук делится на две посылки: первая в трёх пакетах (с чужим пакетом между ними)
        let first = pes(&audio[..300], 5);
        ts.extend(packet(0x101, true, &first[..170]));
        ts.extend(packet(0x102, false, b"zzzz"));
        ts.extend(packet(0x101, false, &first[170..]));
        ts.extend(packet(0x101, true, &pes(&audio[300..], 0)));
        (ts, audio)
    }

    #[test]
    fn extracts_mpeg_audio_and_skips_other_tracks() {
        let (ts, audio) = sample_ts(0x03);
        assert!(looks_like_ts(&ts));
        let (es, kind) = extract_audio(&ts).unwrap();
        assert_eq!(kind, TsAudio::Mpeg);
        assert_eq!(es, audio);
    }

    #[test]
    fn detects_aac_adts() {
        let (ts, audio) = sample_ts(0x0F);
        let (es, kind) = extract_audio(&ts).unwrap();
        assert_eq!(kind, TsAudio::AacAdts);
        assert_eq!(es, audio);
    }

    #[test]
    fn rejects_unsupported_and_broken_input() {
        assert!(!looks_like_ts(b"ID3 not a ts"));
        assert!(extract_audio(b"hello").is_err());
        // Тип 0x11 (LATM) отклоняем с понятным текстом
        let (ts, _) = sample_ts(0x11);
        assert!(extract_audio(&ts).unwrap_err().contains("0x11"));
        // Видео без звука
        let (ts, _) = sample_ts(0x1B);
        assert!(extract_audio(&ts).unwrap_err().contains("не найдена"));
        // Сломанный синхробайт в середине
        let (mut ts, _) = sample_ts(0x03);
        ts[PACKET * 4] = 0;
        assert!(extract_audio(&ts).unwrap_err().contains("синхробайт"));
    }
}
