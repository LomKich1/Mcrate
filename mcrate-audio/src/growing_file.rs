//! Читатель файла, который ещё докачивается.
//!
//! Обычный файл при чтении «за концом» просто говорит «всё, конец». Для докачиваемого файла
//! это неверно: конец пока не настоящий, данные просто ещё не пришли. Поэтому `GrowingFile`
//! перед каждым чтением спрашивает `DownloadProgress`, сколько байт уже записано, и если
//! нужных ещё нет, ЖДЁТ (спит на «звонке»), а не возвращает конец.
//!
//! Конец файла (EOF) читатель сообщает только в двух случаях: загрузка успешно закончилась
//! и все байты прочитаны, либо её отменили (тогда декодер аккуратно заканчивает трек).
//! Ошибка загрузки превращается в ошибку чтения.
//!
//! Важно: читает этот файл декодер внутри звукового потока rodio. Пока данных нет, звук
//! молчит (это и есть «буферизация»); команды движка при этом тоже могут подвиснуть до прихода
//! данных, поэтому движок начинает играть, только когда скачан запас (см. `START_BYTES` в engine.rs).

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

use mcrate_net::{Available, DownloadProgress};

/// Как часто просыпаемся проверить, не отменили ли загрузку (по звонку просыпаемся сразу,
/// это лишь страховка).
const WAIT_STEP: Duration = Duration::from_millis(100);

/// Сколько первых байт читаем, чтобы по ним узнать формат (см. `looks_like_mp4`).
pub(crate) const HEADER_LEN: usize = 12;

pub(crate) struct GrowingFile {
    file: File,
    /// Наша позиция чтения (ведём сами: нужно знать, чего ждать).
    pos: u64,
    progress: DownloadProgress,
    /// Первые байты файла (для определения формата).
    header: [u8; HEADER_LEN],
}

impl GrowingFile {
    /// Открывает файл. Сначала пробуем `part` (файл докачивается под этим именем), а если его
    /// уже переименовали в `dest` (загрузка успела закончиться), то `dest`.
    /// К этому моменту в файле должно быть хотя бы `HEADER_LEN` байт (иначе формат не определить).
    pub(crate) fn open(part: &Path, dest: &Path, progress: DownloadProgress) -> io::Result<Self> {
        let mut file = File::open(part).or_else(|_| File::open(dest))?;
        let mut header = [0u8; HEADER_LEN];
        let n = file.read(&mut header)?; // меньше HEADER_LEN можно получить только у крошечного файла
        header[n..].fill(0);
        file.seek(SeekFrom::Start(0))?;
        Ok(Self { file, pos: 0, progress, header })
    }

    /// Файл начинается как MP4 (блок `ftyp` на байтах 4..8)? Для такого файла нельзя включать
    /// режим «seekable» у декодера: он потребует длину и прочитает файл до самого конца
    /// (см. описание `AudioPlayer::play_stream`).
    pub(crate) fn looks_like_mp4(&self) -> bool {
        &self.header[4..8] == b"ftyp"
    }
}

impl Read for GrowingFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            // Нужен байт с номером pos, то есть записано должно быть не меньше pos + 1
            match self.progress.wait_for_bytes(self.pos + 1, WAIT_STEP) {
                Available::Data(written) => {
                    let n = (written - self.pos).min(buf.len() as u64) as usize; // не дальше записанного
                    let read = self.file.read(&mut buf[..n])?;
                    self.pos += read as u64;
                    return Ok(read);
                }
                Available::Eof | Available::Cancelled => return Ok(0), // конец (или отмена = «закончили»)
                Available::Failed(message) => return Err(io::Error::other(message)),
                Available::Timeout => continue, // ничего нового, ждём дальше
            }
        }
    }
}

impl Seek for GrowingFile {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target: i128 = match to {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(delta) => i128::from(self.pos) + i128::from(delta),
            SeekFrom::End(delta) => {
                // Длина известна только когда всё скачано
                if !self.progress.is_finished() {
                    return Err(io::Error::new(io::ErrorKind::Unsupported, "длина файла ещё неизвестна: он докачивается"));
                }
                i128::from(self.progress.written()) + i128::from(delta)
            }
        };
        if target < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "позиция до начала файла"));
        }
        let target = target as u64;
        // Перемотка вперёд за скачанное разрешена: следующее чтение подождёт данные
        self.file.seek(SeekFrom::Start(target))?;
        self.pos = target;
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::thread;
    use std::time::Instant;

    /// Временная папка для теста (удаляется при выходе из области видимости).
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("mcrate-growing-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn path(&self, file: &str) -> PathBuf {
            self.0.join(file)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Имитация загрузчика: дописывает данные кусками и сообщает прогрессу.
    fn spawn_writer(path: PathBuf, data: Vec<u8>, progress: DownloadProgress, chunk: usize, pause_ms: u64) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let mut file = File::create(&path).unwrap();
            for part in data.chunks(chunk) {
                file.write_all(part).unwrap();
                progress.add_bytes(part.len() as u64);
                thread::sleep(Duration::from_millis(pause_ms));
            }
            progress.finish();
        })
    }

    fn sample_data(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 % 251) as u8).collect()
    }

    #[test]
    fn reads_everything_while_file_grows() {
        let dir = TempDir::new("grow");
        let part = dir.path("a.part");
        let data = sample_data(20_000);
        File::create(&part).unwrap(); // файл должен существовать до открытия читателем
        let progress = DownloadProgress::new();
        let writer = spawn_writer(part.clone(), data.clone(), progress.clone(), 1000, 3);

        let mut reader = GrowingFile::open(&part, &part, progress).unwrap();
        let mut got = Vec::new();
        reader.read_to_end(&mut got).unwrap(); // дойдёт до конца только когда загрузка закончится
        writer.join().unwrap();
        assert_eq!(got, data);
    }

    #[test]
    fn read_waits_instead_of_returning_eof() {
        let dir = TempDir::new("wait");
        let part = dir.path("a.part");
        fs::write(&part, sample_data(100)).unwrap();
        let progress = DownloadProgress::new();
        progress.add_bytes(100);
        let mut reader = GrowingFile::open(&part, &part, progress.clone()).unwrap();
        let mut buf = [0u8; 100];
        reader.read_exact(&mut buf).unwrap();

        // Всё записанное прочитано, загрузка идёт: следующее чтение должно ПОДОЖДАТЬ, а не вернуть 0
        let p2 = progress.clone();
        let part2 = part.clone();
        let late = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            let mut f = fs::OpenOptions::new().append(true).open(&part2).unwrap();
            f.write_all(&[9, 9, 9]).unwrap();
            p2.add_bytes(3);
        });
        let started = Instant::now();
        let mut more = [0u8; 8];
        let n = reader.read(&mut more).unwrap();
        assert_eq!((n, &more[..n]), (3, &[9u8, 9, 9][..]));
        assert!(started.elapsed() >= Duration::from_millis(100), "читатель не подождал данные");
        late.join().unwrap();
    }

    #[test]
    fn cancel_wakes_reader_with_eof() {
        let dir = TempDir::new("cancel");
        let part = dir.path("a.part");
        File::create(&part).unwrap();
        let progress = DownloadProgress::new();
        let mut reader = GrowingFile::open(&part, &part, progress.clone()).unwrap();
        let p2 = progress.clone();
        let t = thread::spawn(move || {
            thread::sleep(Duration::from_millis(80));
            p2.cancel();
        });
        let started = Instant::now();
        let mut buf = [0u8; 16];
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        assert!(started.elapsed() < Duration::from_secs(3));
        t.join().unwrap();
    }

    #[test]
    fn download_error_becomes_read_error_after_data() {
        let dir = TempDir::new("fail");
        let part = dir.path("a.part");
        fs::write(&part, sample_data(10)).unwrap();
        let progress = DownloadProgress::new();
        progress.add_bytes(10);
        progress.fail("обрыв сети".to_string());
        let mut reader = GrowingFile::open(&part, &part, progress).unwrap();
        let mut buf = [0u8; 10];
        reader.read_exact(&mut buf).unwrap(); // то, что успело скачаться, ещё читается
        let err = reader.read(&mut buf).unwrap_err();
        assert!(err.to_string().contains("обрыв сети"));
    }

    #[test]
    fn seek_rules() {
        let dir = TempDir::new("seek");
        let part = dir.path("a.part");
        let data = sample_data(1000);
        fs::write(&part, &data[..600]).unwrap();
        let progress = DownloadProgress::new();
        progress.add_bytes(600);
        let mut reader = GrowingFile::open(&part, &part, progress.clone()).unwrap();

        // внутри скачанного: назад и вперёд
        assert_eq!(reader.seek(SeekFrom::Start(500)).unwrap(), 500);
        let mut b = [0u8; 4];
        reader.read_exact(&mut b).unwrap();
        assert_eq!(&b, &data[500..504]);
        assert_eq!(reader.seek(SeekFrom::Current(-4)).unwrap(), 500);
        assert_eq!(reader.seek(SeekFrom::Start(0)).unwrap(), 0);
        // от конца, пока длина неизвестна: отказ (а не ложь)
        assert_eq!(reader.seek(SeekFrom::End(0)).unwrap_err().kind(), io::ErrorKind::Unsupported);
        // до начала файла нельзя
        assert!(reader.seek(SeekFrom::Current(-1)).is_err());

        // за скачанное можно: чтение подождёт
        reader.seek(SeekFrom::Start(700)).unwrap();
        let rest = data[600..].to_vec();
        let (part2, p2) = (part.clone(), progress.clone());
        let late = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            let mut f = fs::OpenOptions::new().append(true).open(&part2).unwrap();
            f.write_all(&rest).unwrap();
            p2.add_bytes(400);
            p2.finish();
        });
        reader.read_exact(&mut b).unwrap();
        assert_eq!(&b, &data[700..704]);
        late.join().unwrap();

        // загрузка закончилась: длина известна
        assert_eq!(reader.seek(SeekFrom::End(0)).unwrap(), 1000);
        assert_eq!(reader.seek(SeekFrom::End(-10)).unwrap(), 990);
    }

    #[test]
    fn detects_mp4_by_ftyp_box() {
        let dir = TempDir::new("mp4");
        let part = dir.path("a.part");
        fs::write(&part, [0, 0, 0, 0x1c, b'f', b't', b'y', b'p', b'i', b's', b'o', b'5', 1, 2, 3]).unwrap();
        let reader = GrowingFile::open(&part, &part, DownloadProgress::new()).unwrap();
        assert!(reader.looks_like_mp4());
        fs::write(&part, b"ID3\x03\x00\x00\x00\x00\x00\x00\x00\x00\x00").unwrap();
        let reader = GrowingFile::open(&part, &part, DownloadProgress::new()).unwrap();
        assert!(!reader.looks_like_mp4());
    }

    #[test]
    fn open_falls_back_to_final_name() {
        let dir = TempDir::new("rename");
        let dest = dir.path("a.m4a");
        fs::write(&dest, sample_data(50)).unwrap();
        // файла .part уже нет (его переименовали): должны открыть итоговый
        assert!(GrowingFile::open(&dir.path("a.m4a.part"), &dest, DownloadProgress::new()).is_ok());
    }
}
