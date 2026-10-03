# Quiet Instrumentation

## Visual Philosophy

Quiet Instrumentation treats a desktop control surface as an instrument panel, not a settings form. Its geometry is precise and legible, with a strong frame, measured margins, and a quiet navigation rail. Space separates distinct systems; alignment makes their relationships immediately apparent.

The material is nocturnal graphite with a faint blue-violet cast. Cool periwinkle marks focus, selection, and active state; muted steel carries secondary information. Color is functional before it is decorative. Thin borders and restrained highlights lend the interface a crafted, machined quality without turning it into a dashboard full of chrome.

Typography is monospaced, but its rhythm is editorial: compact labels, clear section titles, and numbers that read like instrument values. Essential words only. Status is communicated through position, shape, and contrast as much as through text, making the interface equally at home under a pointer or a keyboard focus ring.

Normal and Pixel are two finishes applied to one architecture. Normal favors softened contrast and calmer surfaces; Pixel sharpens corners, edges, and typographic character. Neither changes the information hierarchy. Every control, transition, and alignment should feel meticulously crafted, the product of patient refinement and deep expertise rather than ornament added after the fact.

## Concept

The interface is a small observatory for the desktop: a stable frame around systems that are usually invisible. The active page occupies the instrument's main field; the navigation rail acts as its index. A slim top status strip signals the current visual mode and whether changes are active. The footer anchors keyboard guidance without competing with the controls.

## Layout Direction

- Wide window, approximately 960 x 680 logical pixels.
- Left navigation rail, approximately 190 pixels; right content field fills the remaining width.
- Top identity/status strip, page title and short contextual caption.
- Main content uses compact grouped surfaces, not a grid of oversized cards.
- Footer keeps keyboard hints visible; pointer targets remain generous.
- One shared component structure supports Normal and Pixel finishes.

## Interaction Direction

- Navigation works by click and arrow/Enter keys.
- Every pointer action has a keyboard equivalent and visible focus.
- Module order supports drag-and-drop plus explicit move earlier/later controls.
- Language switch lives in a predictable global location; default remains Russian.
- Prefer immediate application with a clear transient status over a hidden save state.

## First Screen: Panel

- Sidebar: Обзор, Панель, Внешний вид, Уведомления, Управление.
- Header: Панель / short description / active-module count.
- Height control: one horizontal slider with exact numeric value and step buttons.
- Modules: two compact columns of rows with icon or marker, localized name, enabled state, and drag handle.
- A small order preview communicates that module order affects the bar.
- Footer: concise mouse and keyboard affordances, close action.

## Palette

- Canvas: `#12151d` to `#171b25`.
- Surface: `#1b202b`.
- Hairline: `#343c4c`.
- Primary text: `#dee2ef`.
- Secondary text: `#9299aa`.
- Accent: `#a7c8ff`.
- Accent glow: reserved for focus and active selection only.

## Not in the First Mockup

- No charts, graphs, decorative sci-fi reticles, or fake telemetry.
- No multiple competing accent colors.
- No two separate layouts for Normal and Pixel.
- No implementation changes before the visual direction is approved.
