//! Общий plumbing layer-shell окна: поверхность, пул буферов, масштаб,
//! применение configure без нулевых размеров и доставка кадра в слой.
//!
//! Вынесено из `hud_menu.rs`, чтобы и меню, и wallpaper-picker строили
//! surface/pool/configure/present одинаково, без копирования.

use smithay_client_toolkit::{
    compositor::{CompositorState, SurfaceData},
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceData,
        },
    },
    shm::{Shm, slot::SlotPool},
};
use wayland_client::{Dispatch, QueueHandle, protocol::wl_surface};

/// Контекст живого слоя: та же поверхность на весь цикл, пул буферов и
/// параметры рендера, которые раньше висели на `MenuApp` по отдельности.
pub struct Context {
    pub layer: LayerSurface,
    pub pool: SlotPool,
    pub width: u32,
    pub height: u32,
    pub configured: bool,
    pub dirty: bool,
    pub scale: u32,
}

impl Context {
    /// Применяет configure от композитора. Нули отбрасываются: слой без
    /// якорей не имеет права получить нулевой размер — это ошибка протокола.
    /// Если после фильтра нашли ноль — берём рабочую просьбу модели.
    pub fn apply_configure(&mut self, proposed: (u32, u32), fallback: (u32, u32)) {
        if proposed.0 > 0 {
            self.width = proposed.0;
        }
        if proposed.1 > 0 {
            self.height = proposed.1;
        }
        if self.width == 0 || self.height == 0 {
            self.width = fallback.0;
            self.height = fallback.1;
        }
        self.configured = true;
        self.dirty = true;
    }

    /// Запрос размера у композитора. Повторный `set_size` с той же величиной
    /// не шлём: лишний commit лишь трёт канал.
    pub fn request_size(&mut self, width: u32, height: u32) {
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        self.layer.set_size(width, height);
        self.layer.commit();
        self.dirty = true;
    }

    /// Рендерит пик-карту через `paint` и отдаёт её слою. Возвращает false,
    /// если кадр не доставлен (пустые размеры, пул занят): вызывающий ставит
    /// dirty и попробует на следующем тике.
    pub fn present<F>(&mut self, paint: F) -> bool
    where
        F: FnOnce(&mut tiny_skia::Pixmap),
    {
        if self.width == 0 || self.height == 0 {
            return false;
        }
        let pw = (self.width as f32 * self.scale as f32) as u32;
        let ph = (self.height as f32 * self.scale as f32) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return false;
        };
        paint(&mut pixmap);

        let stride = (pw * 4) as i32;
        let Ok((buffer, canvas)) = self.pool.create_buffer(
            pw as i32,
            ph as i32,
            stride,
            wayland_client::protocol::wl_shm::Format::Argb8888,
        ) else {
            self.dirty = true;
            return false;
        };
        let (dst, _) = canvas.as_chunks_mut::<4>();
        let (src, _) = pixmap.data().as_chunks::<4>();
        for (out, input) in dst.iter_mut().zip(src.iter()) {
            *out = [input[2], input[1], input[0], input[3]];
        }
        let surface = self.layer.wl_surface();
        surface.set_buffer_scale(self.scale as i32);
        surface.damage_buffer(0, 0, pw as i32, ph as i32);
        let _ = buffer.attach_to(surface);
        self.layer.commit();
        true
    }
}

/// Параметры окна слоя: имя пространства, размер и высота под пул буферов.
/// Собраны в структуру, потому что иначе список аргументов перестаёт читаться.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    pub namespace: Option<&'static str>,
    pub width: u32,
    pub height: u32,
    /// Потолок высоты: по нему считается пул буферов.
    pub max_height: u32,
    /// Плотность буфера: ровно та, что у `set_buffer_scale`.
    pub scale: u32,
}

/// Создаёт слой: поверхность, `create_layer_surface`, `set_size` до первого
/// commit и пул буферов под `shape.max_height`. Поведение = стартап hud-menu.
pub fn open<D>(
    compositor: &CompositorState,
    layer_shell: &LayerShell,
    shm: &Shm,
    qh: &QueueHandle<D>,
    shape: Shape,
) -> Result<Context, String>
where
    D: Dispatch<wl_surface::WlSurface, SurfaceData<()>>
        + Dispatch<
            smithay_client_toolkit::reexports::protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
            LayerSurfaceData,
        >
        + LayerShellHandler
        + 'static,
{
    let surface = compositor.create_surface(qh);
    let layer = layer_shell.create_layer_surface(
        qh,
        surface.clone(),
        Layer::Overlay,
        shape.namespace,
        None,
    );
    layer.set_anchor(Anchor::empty());
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.set_exclusive_zone(-1);
    layer.set_margin(0, 0, 0, 0);
    // Размер до первого commit: у слоя без якорей композитор не возьмёт
    // ширину откуда-то ещё.
    layer.set_size(shape.width, shape.height);
    layer.commit();
    let bytes = shape.width as usize
        * shape.max_height as usize
        * 4
        * shape.scale as usize
        * shape.scale as usize;
    let pool = SlotPool::new(bytes * 3 + 4_000_000, shm)
        .map_err(|error| format!("не создать пул буферов: {error}"))?;
    Ok(Context {
        layer,
        pool,
        width: shape.width,
        height: shape.height,
        configured: false,
        dirty: true,
        scale: shape.scale,
    })
}
