//! Блокировки, переживающие отравление.
//!
//! `Mutex` помечается отравленным, если поток, державший блокировку, упал.
//! Это полезно как сигнал: данные могли остаться недописанными. Но `unwrap()`
//! на такой блокировке превращает любую одну панику в фоновом потоке в панику
//! читателя — и панель, отрисовка которой идёт в другом потоке, перестаёт
//! рисоваться целиком.
//!
//! Здесь блокировка берётся всегда: отравленный guard отдаётся через
//! `PoisonError::into_inner`. Данные при этом те же, что успели записать, —
//! семантика не меняется, меняется только то, что паника в одном потоке не
//! убивает читателей.
//!
//! Заметьте: это не делает состояние корректным после паники. Правило
//! остаётся прежним — писать в блокировке нужно без паники, а
//! `PoisonError` сигнализирует, что где-то это правило нарушили.

use std::sync::{Mutex, MutexGuard};

/// Взять `Mutex` даже если он отравлен.
pub fn mutex<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Паника внутри блокировки оставляет её отравленной — именно этот случай
    /// и роняет `unwrap()` у читателя.
    #[test]
    fn a_poisoned_mutex_is_still_readable() {
        let lock = Mutex::new(7i32);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut guard = lock.lock().unwrap();
            *guard += 1;
            panic!("падаем, держа блокировку");
        }));

        assert!(lock.lock().is_err(), "блокировка отравлена");
        assert_eq!(*mutex(&lock), 8, "значение, записанное до паники, на месте");
    }

    #[test]
    fn a_healthy_lock_behaves_exactly_as_before() {
        let plain = Mutex::new(String::from("ok"));
        assert_eq!(mutex(&plain).as_str(), "ok");
    }
}
