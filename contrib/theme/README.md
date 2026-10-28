# Тема HUDbar в matugen

Здесь лежат файлы, которые связывают matugen с GTK4/libadwaita, GTK3 и
экран входа. Полная матрица компонентов и объяснение решений —
`docs/theme.md`.

## GTK4 / libadwaita

`matugen-gtk4.css` — шаблон matugen. libadwaita 1.10 берёт акцент и фон из
переменных `:root`, а не из `@define-color`; шаблон задаёт обе формы.

```fish
cp matugen-gtk4.css ~/.config/matugen/templates/
```

В `~/.config/matugen/config.toml` шаблон GTK4 должен смотреть на него:

```toml
[templates.gtk4]
input_path = '~/.config/matugen/templates/matugen-gtk4.css'
output_path = '~/.config/gtk-4.0/colors.css'
```

## GTK3 и GTK4: импорт

matugen пишет `colors.css`, но GTK читает `gtk.css`. Положите в
`~/.config/gtk-3.0/gtk.css` и `~/.config/gtk-4.0/gtk.css` одну строку:

```css
@import url("colors.css");
```

Файл `gtk.css` — ваш, его matugen не перезаписывает; поэтому импорт не
теряется при смене обоев.

## Экран входа

Основной экран входа — `hud-greeter-rs`, свой гритер на примитивах Hudbar:
палитра и обои переносятся в его каталог командой `hud-greeter-rs --sync`.
Устройство, установка и откат — `docs/greeter.md`; конфиги niri и greetd —
`contrib/greeter/`.

`tuigreet.toml` остаётся запасным вариантом: статическая тема для `tuigreet`.
Динамической она быть не может — `/etc/tuigreet/config.toml` принадлежит
root, а matugen работает от пользователя. Установка (нужен root):

```fish
sudo install -Dm644 tuigreet.toml /etc/tuigreet/config.toml
```

После этого цвета запасного входа не следуют за обоями, но совпадают с
базовой тёмной палитрой HUDbar. Подробнее — `docs/theme.md`.

## Qt

Qt-приложения не требуют qt6ct: достаточно, чтобы сессия выставляла
`QT_QPA_PLATFORMTHEME=gtk3`. В niri это строка в блоке `environment`:

```kdl
environment {
    QT_QPA_PLATFORMTHEME "gtk3"
}
```
