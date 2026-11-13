// Windows: без консольного окна (приложение с окном; в отладочной сборке консоль остаётся — видны сообщения)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! Запуск на компьютере. Всё приложение — в библиотеке (lib.rs): её же собирает Android (mobile_entry_point).

fn main() {
    vkmusic_lib::run()
}
