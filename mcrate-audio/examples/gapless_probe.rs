//! Пробник для шага 4 («бесшовность»): как ведёт себя очередь `rodio::Player` на стыке двух треков.
//! Звуковая карта не нужна: берём `Player::new()` (он отдаёт «выход очереди») и сами вычитываем
//! сэмплы по одному, как это делает микшер. Всё считается в сэмплах, а не в секундах, поэтому
//! результат не зависит от скорости машины.
//!
//! Запуск: `cargo run -p mcrate-audio --example gapless_probe`

use std::num::NonZero;

use rodio::Player;
use rodio::buffer::SamplesBuffer;

const RATE: u32 = 44_100;
const SECONDS: usize = 1;
const TRACK_SAMPLES: usize = RATE as usize * 2 * SECONDS; // стерео: 2 сэмпла на кадр

/// Трек из одного постоянного значения: по нему сразу видно, где кончился один и начался другой.
fn constant_track(value: f32) -> SamplesBuffer {
    let channels = NonZero::new(2u16).unwrap();
    let rate = NonZero::new(RATE).unwrap();
    SamplesBuffer::new(channels, rate, vec![value; TRACK_SAMPLES])
}

fn ms(samples: usize) -> f32 {
    samples as f32 / 2.0 / RATE as f32 * 1000.0
}

fn main() {
    // E1: второй трек дописан в очередь, пока первый ещё играет
    {
        let (player, mut out) = Player::new();
        player.append(constant_track(0.5));
        player.append(constant_track(0.25));
        let first: Vec<f32> = out.by_ref().take(TRACK_SAMPLES).collect();
        let second: Vec<f32> = out.by_ref().take(TRACK_SAMPLES).collect();
        let zeros = first.iter().chain(&second).filter(|v| **v == 0.0).count();
        println!(
            "E1 стык в одной очереди: тишины между треками {zeros} сэмплов; конец 1-го = {}, начало 2-го = {}",
            first[TRACK_SAMPLES - 1],
            second[0]
        );
        println!("   все сэмплы 1-го на месте: {}, 2-го: {}", first.iter().all(|v| *v == 0.5), second.iter().all(|v| *v == 0.25));
    }

    // E2: громкость у Player одна на всю очередь. Меняем её ровно на стыке. Это лучший случай:
    // движок узнаёт о стыке опросом раз в 100 мс и так точно попасть не может
    {
        let (player, mut out) = Player::new();
        player.append(constant_track(1.0));
        player.append(constant_track(1.0));
        let _: Vec<f32> = out.by_ref().take(TRACK_SAMPLES).collect(); // дошли до самого стыка
        player.set_volume(0.5); // «у нового трека своё усиление»
        let head: Vec<f32> = out.by_ref().take(TRACK_SAMPLES / 2).collect();
        let stale = head.iter().take_while(|v| **v == 1.0).count();
        println!(
            "E2 громкость, смена ровно на стыке (лучший случай): {stale} сэмплов ({:.1} мс) нового трека со старой громкостью",
            ms(stale)
        );
        // Промах движка на один опрос: стык уже прошёл, а громкость меняем только сейчас
        let (player, mut out) = Player::new();
        player.append(constant_track(1.0));
        player.append(constant_track(1.0));
        let late = RATE as usize * 2 / 10; // 100 мс после стыка
        let _: Vec<f32> = out.by_ref().take(TRACK_SAMPLES + late).collect();
        player.set_volume(0.5);
        println!("   при опоздании на один опрос (100 мс) первые {:.0} мс нового трека звучат со старой громкостью", ms(late));
    }

    // E3: что видно снаружи после стыка (так движок узнает о смене трека)
    {
        let (player, mut out) = Player::new();
        player.append(constant_track(0.5));
        player.append(constant_track(0.25));
        let _: Vec<f32> = out.by_ref().take(TRACK_SAMPLES / 2).collect();
        println!("E3 до стыка: len = {}, позиция = {:?}", player.len(), player.get_pos());
        let _: Vec<f32> = out.by_ref().take(TRACK_SAMPLES / 2 + 4410).collect(); // стык и ещё 50 мс
        println!("   после стыка (+50 мс): len = {}, позиция = {:?}", player.len(), player.get_pos());
    }

    // E5: у треков разная частота и число каналов, выход один (44,1 кГц стерео), как в микшере rodio
    {
        let (mixer, mut mix_out) = rodio::mixer::mixer(NonZero::new(2u16).unwrap(), NonZero::new(RATE).unwrap());
        let player = Player::connect_new(&mixer);
        player.append(constant_track(0.5)); // 44,1 кГц, стерео
        let mono_48k = SamplesBuffer::new(NonZero::new(1u16).unwrap(), NonZero::new(48_000u32).unwrap(), vec![0.25; 48_000]);
        player.append(mono_48k); // 48 кГц, моно, 1 с
        let all: Vec<f32> = mix_out.by_ref().take(TRACK_SAMPLES * 2 + 2000).collect();
        // Микшер сначала выдаёт несколько нулей (прогрев), поэтому отсчитываем от первого сэмпла 1-го трека
        let start = all.iter().position(|v| (*v - 0.5).abs() < 0.01).unwrap();
        let boundary = start + all[start..].iter().position(|v| (*v - 0.5).abs() > 0.01).unwrap();
        let zeros = all[start..boundary + 400].iter().filter(|v| **v == 0.0).count();
        println!(
            "E5 разная частота и каналы: прогрев микшера {start} сэмплов, 1-й трек {} сэмплов (ждали {TRACK_SAMPLES}), нулей на стыке {zeros}",
            boundary - start
        );
        println!("   значения вокруг стыка: {:?}", &all[boundary - 3..boundary + 9]);
    }

    // E4: нижняя граница для любого подхода, где следующий трек дописывают ПОСЛЕ конца предыдущего
    {
        let (player, mut out) = Player::new();
        player.append(constant_track(0.5));
        let _: Vec<f32> = out.by_ref().take(TRACK_SAMPLES).collect();
        let _: Vec<f32> = out.by_ref().take(100).collect(); // уже идёт «тишина-заглушка» очереди
        player.append(constant_track(0.25));
        let silent = out.by_ref().take_while(|v| *v == 0.0).count();
        println!("E4 дописали через 100 сэмплов после конца: до нового трека ещё {silent} сэмплов тишины ({:.1} мс)", ms(silent));
    }
}
