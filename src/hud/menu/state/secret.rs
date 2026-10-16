use super::{CommandJob, Menu, Outcome, Status};

impl Menu {
    /// Открыт ли ввод секрета.
    pub fn secret_active(&self) -> bool {
        self.secret_input.is_some()
    }

    /// Текущий ввод секрета: нужно окну для отрисовки и тестам.
    pub fn secret_input(&self) -> Option<&super::super::secret::SecretInput> {
        self.secret_input.as_ref()
    }

    /// Открыть ввод секрета для цели. Запрос уровня не трогается: он остаётся
    /// ровно тем, чем был до открытия пароля.
    pub fn open_secret_input(&mut self, target: super::super::secret::SecretTarget) {
        let lang = self.lang;
        self.secret_input = Some(super::super::secret::SecretInput::new(target, lang));
        self.status = None;
    }

    /// Дописать символ к паролю. Любой печатный Unicode: пароль не латиница.
    pub fn type_secret_char(&mut self, ch: char) {
        if let Some(input) = &mut self.secret_input {
            input.push(ch);
        }
    }

    /// Стереть последний символ пароля.
    pub fn secret_backspace(&mut self) {
        if let Some(input) = &mut self.secret_input {
            input.erase();
        }
    }

    /// Очистить пароль целиком (`Ctrl+U`).
    pub fn secret_clear(&mut self) {
        if let Some(input) = &mut self.secret_input {
            input.clear();
        }
    }

    /// Отменить ввод: буфер выбрасывается, команда не составляется, меню
    /// остаётся на том же уровне с той же выбранной строкой.
    pub fn cancel_secret_input(&mut self) -> Outcome {
        match self.secret_input.take() {
            Some(_) => Outcome::Value,
            None => Outcome::Idle,
        }
    }

    /// Подтвердить ввод: собрать команду с `CmdArg::Secret`, поставить её в
    /// очередь worker-а и пометить строку занятой.
    ///
    /// UI-буфер пароля после этого пуст: `SecretInput` разобран на части и
    /// выброшен. Но пароль не «исчез полностью»: он временно живёт внутри
    /// `CommandSpec`, затем в `CommandJob` очереди и, наконец, в `argv`
    /// дочернего процесса (виден через `ps`). От журналов, статусов, `Debug`
    /// и кадров он при этом скрыт — см. `CmdArg::Secret`.
    ///
    /// Пустой пароль не запрещается: правила паролей у NetworkManager свои,
    /// и выдумывать тут нечего.
    pub fn submit_secret_input(&mut self) -> Outcome {
        let Some(input) = self.secret_input.take() else {
            return Outcome::Idle;
        };
        let (target, password) = input.into_parts();
        let command = target.command(&password);
        drop(password);
        self.command_generation += 1;
        let generation = self.command_generation;
        let row = target.row_id().to_string();
        self.busy.insert(row.clone());
        self.pending_commands.push(CommandJob {
            generation,
            commands: vec![command],
            refresh: target.provider_key(),
            row,
        });
        // Статус берётся из `log_line`, где секрет уже `<redacted>`: пароль в
        // подвале появиться не может.
        self.status = Some(Status::Applied(
            self.pending_commands
                .last()
                .and_then(|job| {
                    job.commands
                        .last()
                        .map(super::super::system::CommandSpec::log_line)
                })
                .unwrap_or_default(),
        ));
        self.rebuild_current();
        Outcome::Changed
    }
}
