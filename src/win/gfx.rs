//! Risorse GPU: Direct3D 11 → Direct2D per disegnare, una swap chain "per
//! composizione" con canale alfa e DirectComposition per attaccarla alla
//! finestra. Risultato: trasparenza per pixel, antialiasing e nessuna
//! bitmap di finestra gestita dalla CPU.

use std::collections::HashMap;

use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::{BOOL, Error, Interface, PCWSTR, Result, w};

use super::thumbs::THUMB_KEY;
use crate::media::Artwork;

pub struct Gfx {
    pub factory: ID2D1Factory1,
    pub dc: ID2D1DeviceContext,
    pub brush: ID2D1SolidColorBrush,
    pub dash: ID2D1StrokeStyle,
    /// Tratti con estremi arrotondati (archi e anelli).
    pub round: ID2D1StrokeStyle,
    pub fonts: Fonts,
    /// Sfocatura gaussiana per le transizioni; `None` se il sistema non la offre.
    pub blur: Option<ID2D1Effect>,
    /// Sostituzioni dei glifi delle icone (solo per le anteprime fuori da Windows).
    pub icon_map: Option<fn(char) -> char>,
    dwrite: IDWriteFactory,
    output: Output,
    /// Copertine e anteprime già caricate in GPU, per id dell'`Artwork`.
    images: HashMap<u64, ID2D1Bitmap1>,
}

/// Dove finisce il disegno: la finestra (swap chain + DirectComposition) o
/// una bitmap in memoria, per le anteprime (`--snapshot`).
enum Output {
    Window {
        swap_chain: IDXGISwapChain1,
        _composition: (
            IDCompositionDevice,
            IDCompositionTarget,
            IDCompositionVisual,
        ),
    },
    Offscreen {
        target: ID2D1Bitmap1,
        width: u32,
        height: u32,
    },
}

impl Gfx {
    pub fn new(hwnd: HWND, width: u32, height: u32, dpi: f32) -> Result<Self> {
        unsafe {
            let d3d = create_d3d_device()?;
            let dxgi: IDXGIDevice = d3d.cast()?;
            // Una sola immagine in coda: Present(1) aspetta il vsync e fa da metronomo.
            dxgi.cast::<IDXGIDevice1>()?.SetMaximumFrameLatency(1)?;

            let factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dc = factory
                .CreateDevice(&dxgi)?
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            // Il ClearType non funziona su superfici trasparenti.
            dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

            let dxgi_factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width.max(1),
                Height: height.max(1),
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                ..Default::default()
            };
            let swap_chain = dxgi_factory.CreateSwapChainForComposition(&d3d, &desc, None)?;

            let composition: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
            let target = composition.CreateTargetForHwnd(hwnd, true)?;
            let visual = composition.CreateVisual()?;
            visual.SetContent(&swap_chain)?;
            target.SetRoot(&visual)?;
            composition.Commit()?;

            let output = Output::Window {
                swap_chain,
                _composition: (composition, target, visual),
            };
            let gfx = Self::assemble(factory, dc, output, Families::system)?;
            gfx.bind_target(dpi)?;
            Ok(gfx)
        }
    }

    /// Disegno in memoria, per le anteprime: stessa pipeline, nessuna finestra.
    pub fn offscreen(
        width: u32,
        height: u32,
        dpi: f32,
        families: fn(&IDWriteFactory) -> Families,
    ) -> Result<Self> {
        unsafe {
            let d3d = create_d3d_device()?;
            let dxgi: IDXGIDevice = d3d.cast()?;
            let factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dc = factory
                .CreateDevice(&dxgi)?
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let target = offscreen_target(&dc, width, height, dpi)?;
            let output = Output::Offscreen {
                target,
                width,
                height,
            };
            let gfx = Self::assemble(factory, dc, output, families)?;
            gfx.bind_target(dpi)?;
            Ok(gfx)
        }
    }

    fn assemble(
        factory: ID2D1Factory1,
        dc: ID2D1DeviceContext,
        output: Output,
        families: fn(&IDWriteFactory) -> Families,
    ) -> Result<Self> {
        unsafe {
            let brush = dc.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                },
                None,
            )?;
            let dash = ID2D1Factory::CreateStrokeStyle(
                &factory,
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_DASH,
                    dashOffset: 0.0,
                },
                None,
            )?;
            let round = ID2D1Factory::CreateStrokeStyle(
                &factory,
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                },
                None,
            )?;
            let blur = dc.CreateEffect(&CLSID_D2D1GaussianBlur).ok();
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let families = families(&dwrite);
            let fonts = Fonts::new(&dwrite, &families)?;
            Ok(Self {
                factory,
                dc,
                brush,
                dash,
                round,
                fonts,
                blur,
                icon_map: families.icon_map,
                dwrite,
                output,
                images: HashMap::new(),
            })
        }
    }

    /// Da chiamare quando cambiano dimensione o DPI della finestra.
    pub fn resize(&mut self, width: u32, height: u32, dpi: f32) -> Result<()> {
        unsafe {
            self.dc.SetTarget(None);
            match &mut self.output {
                Output::Window { swap_chain, .. } => swap_chain.ResizeBuffers(
                    0,
                    width.max(1),
                    height.max(1),
                    DXGI_FORMAT_UNKNOWN,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )?,
                Output::Offscreen {
                    target,
                    width: w,
                    height: h,
                } => {
                    *target = offscreen_target(&self.dc, width, height, dpi)?;
                    (*w, *h) = (width, height);
                }
            }
        }
        self.bind_target(dpi)
    }

    fn bind_target(&self, dpi: f32) -> Result<()> {
        unsafe {
            let bitmap = match &self.output {
                Output::Window { swap_chain, .. } => {
                    let surface: IDXGISurface = swap_chain.GetBuffer(0)?;
                    let props = D2D1_BITMAP_PROPERTIES1 {
                        pixelFormat: premultiplied_bgra(),
                        dpiX: dpi,
                        dpiY: dpi,
                        bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                        ..Default::default()
                    };
                    self.dc
                        .CreateBitmapFromDxgiSurface(&surface, Some(&props))?
                }
                Output::Offscreen { target, .. } => target.clone(),
            };
            self.dc.SetTarget(&bitmap);
            // Da qui in poi si disegna in DIP: Direct2D applica la scala da solo.
            self.dc.SetDpi(dpi, dpi);
        }
        Ok(())
    }

    pub fn present(&self) -> Result<()> {
        match &self.output {
            Output::Window { swap_chain, .. } => unsafe {
                swap_chain.Present(1, DXGI_PRESENT(0)).ok()
            },
            Output::Offscreen { .. } => Ok(()),
        }
    }

    /// Pixel BGRA (premoltiplicati) dell'ultimo disegno in memoria.
    pub fn read_pixels(&self) -> Result<(u32, u32, Vec<u8>)> {
        let Output::Offscreen {
            target,
            width,
            height,
        } = &self.output
        else {
            return Err(Error::empty());
        };
        unsafe {
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: premultiplied_bgra(),
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            };
            let size = D2D_SIZE_U {
                width: *width,
                height: *height,
            };
            let readback = self.dc.CreateBitmap(size, None, 0, &props)?;
            readback.CopyFromBitmap(None, target, None)?;
            let mapped = readback.Map(D2D1_MAP_OPTIONS_READ)?;
            let row = *width as usize * 4;
            let mut pixels = Vec::with_capacity(row * *height as usize);
            for y in 0..*height as usize {
                let line =
                    std::slice::from_raw_parts(mapped.bits.add(y * mapped.pitch as usize), row);
                pixels.extend_from_slice(line);
            }
            readback.Unmap()?;
            Ok((*width, *height, pixels))
        }
    }

    /// Bitmap GPU per un'immagine; viene caricata una volta sola.
    pub fn image(&mut self, art: &Artwork) -> Result<ID2D1Bitmap1> {
        if let Some(bitmap) = self.images.get(&art.id) {
            return Ok(bitmap.clone());
        }
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: premultiplied_bgra(),
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };
        let bitmap = unsafe {
            self.dc.CreateBitmap(
                D2D_SIZE_U {
                    width: art.width,
                    height: art.height,
                },
                Some(art.pixels.as_ptr().cast()),
                art.width * 4,
                &props,
            )?
        };
        // Delle copertine serve solo l'ultima.
        if art.id & THUMB_KEY == 0 {
            self.images.retain(|id, _| id & THUMB_KEY != 0);
        }
        self.images.insert(art.id, bitmap.clone());
        Ok(bitmap)
    }

    /// Libera le anteprime che non servono più.
    pub fn retain_images(&mut self, keep: impl Fn(u64) -> bool) {
        self.images.retain(|id, _| keep(*id));
    }

    /// Larghezza in DIP di un testo su una riga.
    pub fn measure(&self, text: &str, format: &IDWriteTextFormat) -> f32 {
        self.layout(text, format).map_or(0.0, |layout| unsafe {
            let mut metrics = DWRITE_TEXT_METRICS::default();
            match layout.GetMetrics(&mut metrics) {
                Ok(()) => metrics.widthIncludingTrailingWhitespace,
                Err(_) => 0.0,
            }
        })
    }

    /// Posizione orizzontale (dal bordo sinistro del testo) del cursore dopo `utf16_pos` unità.
    pub fn caret_x(&self, text: &str, format: &IDWriteTextFormat, utf16_pos: u32) -> f32 {
        let Some(layout) = self.layout(text, format) else {
            return 0.0;
        };
        let (mut x, mut y) = (0.0, 0.0);
        let mut metrics = DWRITE_HIT_TEST_METRICS::default();
        unsafe {
            let _ = layout.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
            if layout
                .HitTestTextPosition(utf16_pos, false, &mut x, &mut y, &mut metrics)
                .is_err()
            {
                return 0.0;
            }
        }
        x
    }

    fn layout(&self, text: &str, format: &IDWriteTextFormat) -> Option<IDWriteTextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            self.dwrite
                .CreateTextLayout(&wide, format, 4096.0, 64.0)
                .ok()
        }
    }
}

fn premultiplied_bgra() -> D2D1_PIXEL_FORMAT {
    D2D1_PIXEL_FORMAT {
        format: DXGI_FORMAT_B8G8R8A8_UNORM,
        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
    }
}

unsafe fn offscreen_target(
    dc: &ID2D1DeviceContext,
    width: u32,
    height: u32,
    dpi: f32,
) -> Result<ID2D1Bitmap1> {
    let props = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: premultiplied_bgra(),
        dpiX: dpi,
        dpiY: dpi,
        bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
        ..Default::default()
    };
    let size = D2D_SIZE_U {
        width: width.max(1),
        height: height.max(1),
    };
    unsafe { dc.CreateBitmap(size, None, 0, &props) }
}

/// Scheda video se possibile, altrimenti il rasterizzatore software WARP.
fn create_d3d_device() -> Result<ID3D11Device> {
    let mut last_error = Error::empty();
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let mut device = None;
        let created = unsafe {
            D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )
        };
        match (created, device) {
            (Ok(()), Some(device)) => return Ok(device),
            (Err(e), _) => last_error = e,
            _ => {}
        }
    }
    Err(last_error)
}

/// Tutti i formati di testo usati dall'interfaccia, creati una volta sola.
pub struct Fonts {
    pub title: IDWriteTextFormat,
    pub artist: IDWriteTextFormat,
    pub small: IDWriteTextFormat,
    pub small_right: IDWriteTextFormat,
    pub small_center: IDWriteTextFormat,
    /// Ora nel notch chiuso: +20% rispetto al testo base (13 → 15,6).
    pub clock: IDWriteTextFormat,
    pub date: IDWriteTextFormat,
    pub date_center: IDWriteTextFormat,
    /// Ora in grassetto nell'intestazione del notch aperto.
    pub status_bold: IDWriteTextFormat,
    pub pill: IDWriteTextFormat,
    pub lyric_current: IDWriteTextFormat,
    pub lyric_dim: IDWriteTextFormat,
    pub timer: IDWriteTextFormat,
    pub amount: IDWriteTextFormat,
    pub amount_center: IDWriteTextFormat,
    pub metric: IDWriteTextFormat,
    pub header: IDWriteTextFormat,
    pub body: IDWriteTextFormat,
    /// Come `body` ma senza "…": il campo di testo scorre invece di tagliare.
    pub input: IDWriteTextFormat,
    pub body_center: IDWriteTextFormat,
    pub button: IDWriteTextFormat,
    pub chip: IDWriteTextFormat,
    pub alert_title: IDWriteTextFormat,
    pub alert_detail: IDWriteTextFormat,
    pub value_right: IDWriteTextFormat,
    pub settings_title: IDWriteTextFormat,
    pub section: IDWriteTextFormat,
    pub icon_small: IDWriteTextFormat,
    pub icon: IDWriteTextFormat,
    pub icon_big: IDWriteTextFormat,
    pub icon_huge: IDWriteTextFormat,
    pub icon_xl: IDWriteTextFormat,
    pub icon_tiny: IDWriteTextFormat,
}

/// Famiglie di caratteri dell'interfaccia.
pub struct Families {
    pub text: PCWSTR,
    pub display: PCWSTR,
    pub icons: PCWSTR,
    pub icon_map: Option<fn(char) -> char>,
}

impl Families {
    /// Quelle di Windows: Segoe UI Variable e Fluent Icons su Windows 11,
    /// Segoe UI e MDL2 Assets (stessi codici delle icone) su Windows 10.
    pub fn system(dwrite: &IDWriteFactory) -> Self {
        Self {
            text: pick_family(dwrite, &[w!("Segoe UI Variable Text"), w!("Segoe UI")]),
            display: pick_family(dwrite, &[w!("Segoe UI Variable Display"), w!("Segoe UI")]),
            icons: pick_family(dwrite, &[w!("Segoe Fluent Icons"), w!("Segoe MDL2 Assets")]),
            icon_map: None,
        }
    }
}

impl Fonts {
    fn new(dwrite: &IDWriteFactory, families: &Families) -> Result<Self> {
        let (text, display, icons) = (families.text, families.display, families.icons);

        use DWRITE_TEXT_ALIGNMENT_CENTER as CENTER;
        use DWRITE_TEXT_ALIGNMENT_LEADING as LEFT;
        use DWRITE_TEXT_ALIGNMENT_TRAILING as RIGHT;
        let bold = DWRITE_FONT_WEIGHT_BOLD;
        let semibold = DWRITE_FONT_WEIGHT_SEMI_BOLD;
        let regular = DWRITE_FONT_WEIGHT_NORMAL;
        let make = |family, size, weight, align| make_format(dwrite, family, size, weight, align);

        Ok(Self {
            title: make(text, 13.5, semibold, LEFT)?,
            artist: make(text, 12.0, regular, LEFT)?,
            small: make(text, 10.5, regular, LEFT)?,
            small_right: make(text, 10.5, regular, RIGHT)?,
            small_center: make(text, 10.5, regular, CENTER)?,
            clock: make(text, 15.6, semibold, CENTER)?,
            date: make(text, 12.0, regular, LEFT)?,
            date_center: make(text, 12.0, regular, CENTER)?,
            status_bold: make(text, 12.0, bold, LEFT)?,
            pill: make(text, 13.0, semibold, CENTER)?,
            lyric_current: make(display, 15.0, semibold, CENTER)?,
            lyric_dim: make(text, 12.0, regular, CENTER)?,
            timer: make(display, 38.0, bold, CENTER)?,
            amount: make(display, 28.0, bold, LEFT)?,
            amount_center: make(display, 23.0, bold, CENTER)?,
            metric: make(display, 17.0, semibold, LEFT)?,
            header: make(display, 14.0, bold, LEFT)?,
            body: make(text, 12.5, regular, LEFT)?,
            input: untrimmed(make(text, 12.5, regular, LEFT)?)?,
            body_center: make(text, 11.5, regular, CENTER)?,
            button: make(text, 12.0, semibold, CENTER)?,
            chip: make(text, 10.5, semibold, CENTER)?,
            alert_title: make(text, 14.0, semibold, LEFT)?,
            alert_detail: make(text, 12.5, regular, LEFT)?,
            value_right: make(text, 13.0, semibold, RIGHT)?,
            settings_title: make(display, 26.0, semibold, LEFT)?,
            section: make(text, 12.0, bold, LEFT)?,
            icon_small: make(icons, 11.0, regular, CENTER)?,
            icon: make(icons, 13.5, regular, CENTER)?,
            icon_big: make(icons, 18.0, regular, CENTER)?,
            icon_huge: make(icons, 21.0, regular, CENTER)?,
            icon_xl: make(icons, 26.0, regular, CENTER)?,
            icon_tiny: make(icons, 9.0, regular, CENTER)?,
        })
    }
}

fn make_format(
    dwrite: &IDWriteFactory,
    family: PCWSTR,
    size: f32,
    weight: DWRITE_FONT_WEIGHT,
    align: DWRITE_TEXT_ALIGNMENT,
) -> Result<IDWriteTextFormat> {
    unsafe {
        let format = dwrite.CreateTextFormat(
            family,
            None,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!("it-it"),
        )?;
        format.SetTextAlignment(align)?;
        format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        // Testi troppo lunghi finiscono con "…" invece di uscire dal notch.
        let ellipsis = dwrite.CreateEllipsisTrimmingSign(&format)?;
        let trimming = DWRITE_TRIMMING {
            granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
            delimiter: 0,
            delimiterCount: 0,
        };
        format.SetTrimming(&trimming, &ellipsis)?;
        Ok(format)
    }
}

fn untrimmed(format: IDWriteTextFormat) -> Result<IDWriteTextFormat> {
    let none = DWRITE_TRIMMING {
        granularity: DWRITE_TRIMMING_GRANULARITY_NONE,
        delimiter: 0,
        delimiterCount: 0,
    };
    unsafe { format.SetTrimming(&none, None)? };
    Ok(format)
}

/// Primo font installato tra quelli proposti (l'ultimo fa da ripiego).
pub fn pick_family(dwrite: &IDWriteFactory, names: &[PCWSTR]) -> PCWSTR {
    let mut collection = None;
    unsafe {
        if dwrite
            .GetSystemFontCollection(&mut collection, false)
            .is_ok()
            && let Some(collection) = collection
        {
            for &name in names {
                let (mut index, mut exists) = (0u32, BOOL(0));
                if collection
                    .FindFamilyName(name, &mut index, &mut exists)
                    .is_ok()
                    && exists.as_bool()
                {
                    return name;
                }
            }
        }
    }
    names[names.len() - 1]
}
