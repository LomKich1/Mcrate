use mcrate_core::ProviderKind; // берём тип из нашего крейта mcrate-core

fn main() {
    // println! печатает строку в консоль; {:?} выводит значение в отладочном виде
    println!("Mcrate запускается. Для проверки связи: {:?}", ProviderKind::Yandex);
}