#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::rc::Rc;
use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use defmt_rtt as _;
use panic_probe as _;
// Pulls cortex-m-rt's link.x into the dep graph; required for the entry point
// emitted by `#[embassy_executor::main]` on Cortex-M.
use cortex_m_rt as _;

use embassy_executor::Spawner;
use embassy_time::{Duration, Instant, Timer};

use embedded_hal::spi::SpiBus;

use slint::platform::software_renderer::{
    LineBufferProvider, MinimalSoftwareWindow, RepaintBufferType, Rgb565Pixel,
};
use slint::platform::{Platform, PlatformError, WindowAdapter};
use slint::{ComponentHandle, PhysicalSize};

use ui::AppWindow;

// ---------------------------------------------------------------------------
// Heap. Slint allocates internally, so the heap must come up before any Slint
// call. embedded-alloc 0.7 exposes `LlffHeap` (the previous `Heap` type alias
// is still re-exported as a compatibility shim).
// ---------------------------------------------------------------------------

#[global_allocator]
static HEAP: embedded_alloc::LlffHeap = embedded_alloc::LlffHeap::empty();

const HEAP_SIZE: usize = 192 * 1024;
static mut HEAP_MEM: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

// ---------------------------------------------------------------------------
// Display constants. ST7789 240x240 SPI panel on SPI0.
// Pin numbers are GPIO indices on the RP2350A package.
// ---------------------------------------------------------------------------

const _PIN_SCK:  u8 = 18;
const _PIN_MOSI: u8 = 19;
const _PIN_CS:   u8 = 17;
const _PIN_DC:   u8 = 16;
const _PIN_RST:  u8 = 20;
const _PIN_BL:   u8 = 21;

const DISPLAY_W: u16 = 240;
const DISPLAY_H: u16 = 240;

// ---------------------------------------------------------------------------
// Param mirrors. Callbacks from the Slint UI write here; the dsp_logger task
// reads them. Floats are bit-cast through AtomicU32 so the same shape works
// without std atomics-for-f32.
// ---------------------------------------------------------------------------

static G_GAIN:    AtomicU32  = AtomicU32::new(0x3F000000);  // 0.5
static G_DELAY:   AtomicU32  = AtomicU32::new(0x3E800000);  // 0.25
static G_REVERB:  AtomicU32  = AtomicU32::new(0x3E99999A);  // ~0.3
static G_BYPASS:  AtomicBool = AtomicBool::new(false);

// Stand-in for "the DSP is alive" — incremented every poll.
static DSP_TICK: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Slint Platform glue.
// ---------------------------------------------------------------------------

struct McuPlatform {
    window: Rc<MinimalSoftwareWindow>,
    boot:   Instant,
}

impl Platform for McuPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> core::time::Duration {
        let micros = (Instant::now() - self.boot).as_micros();
        core::time::Duration::from_micros(micros)
    }
}

// ---------------------------------------------------------------------------
// Display driver stub. A real ST7789 implementation is out of scope; this
// only needs to compile clean and shape the line-flush surface.
// ---------------------------------------------------------------------------

struct Display<S: SpiBus<u8>> {
    _spi: S,
}

impl<S: SpiBus<u8>> Display<S> {
    fn flush_line(&mut self, _y: u16, _pixels: &[Rgb565Pixel]) {
        // Real impl: set address window for row `_y`, push pixels (16-bit BE)
        // over the embedded-hal v1 SpiBus. Left as a stub on purpose.
    }
}

struct LineBuf<'a, S: SpiBus<u8>> {
    display: &'a mut Display<S>,
    line:    [Rgb565Pixel; DISPLAY_W as usize],
}

impl<'a, S: SpiBus<u8>> LineBufferProvider for &mut LineBuf<'a, S> {
    type TargetPixel = Rgb565Pixel;

    fn process_line(
        &mut self,
        line: usize,
        range: core::ops::Range<usize>,
        render_fn: impl FnOnce(&mut [Self::TargetPixel]),
    ) {
        let buf = &mut self.line[range.start..range.end];
        render_fn(buf);
        self.display.flush_line(line as u16, buf);
    }
}

// A trivial no-op SPI bus stand-in. Only used so the type plumbing compiles
// without depending on a concrete embassy-rp SPI peripheral configuration —
// real boards would substitute `embassy_rp::spi::Spi<'static, SPI0, Async>`
// and a CS/DC pin pair here.
struct DummySpi;

impl embedded_hal::spi::ErrorType for DummySpi {
    type Error = core::convert::Infallible;
}

impl SpiBus<u8> for DummySpi {
    fn read(&mut self, _w: &mut [u8]) -> Result<(), Self::Error> { Ok(()) }
    fn write(&mut self, _w: &[u8]) -> Result<(), Self::Error> { Ok(()) }
    fn transfer(&mut self, _r: &mut [u8], _w: &[u8]) -> Result<(), Self::Error> { Ok(()) }
    fn transfer_in_place(&mut self, _w: &mut [u8]) -> Result<(), Self::Error> { Ok(()) }
    fn flush(&mut self) -> Result<(), Self::Error> { Ok(()) }
}

// ---------------------------------------------------------------------------
// Periodic "DSP" task. Reads the shared param mirrors, bumps a dummy DSP
// counter, and logs over RTT/defmt. Behaviour parity with the desktop audio
// callback is intentional.
// ---------------------------------------------------------------------------

#[embassy_executor::task]
async fn dsp_logger() {
    loop {
        let gain   = f32::from_bits(G_GAIN.load(Ordering::Relaxed));
        let delay  = f32::from_bits(G_DELAY.load(Ordering::Relaxed));
        let reverb = f32::from_bits(G_REVERB.load(Ordering::Relaxed));
        let bypass = G_BYPASS.load(Ordering::Relaxed);

        let tick = DSP_TICK.fetch_add(1, Ordering::Relaxed).wrapping_add(1);

        defmt::info!(
            "dsp tick={=u32} gain={=f32} delay={=f32} reverb={=f32} bypass={=bool}",
            tick, gain, delay, reverb, bypass
        );

        Timer::after(Duration::from_millis(500)).await;
    }
}

// ---------------------------------------------------------------------------
// Entry point.
//
// Pico 2 W note: the onboard LED is wired through the CYW43439 wireless chip,
// not GPIO 25. Toggling GPIO 25 on Pico 2 W is a no-op for "status LED"; LED
// indication requires the cyw43 driver, which is out of scope here.
// ---------------------------------------------------------------------------

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // 1) Heap up first — every Slint allocation depends on it.
    unsafe {
        HEAP.init(core::ptr::addr_of_mut!(HEAP_MEM) as usize, HEAP_SIZE);
    }

    // 2) RP2350 peripherals. The default Config is fine for this demo; clocks
    //    come up at the chip default 150 MHz system clock.
    let _p = embassy_rp::init(Default::default());

    // 3) Slint platform. MinimalSoftwareWindow drives the software renderer
    //    in ReusedBuffer mode, which only needs one full-line scratch buffer.
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    window.set_size(PhysicalSize::new(DISPLAY_W as u32, DISPLAY_H as u32));

    let platform = McuPlatform {
        window: window.clone(),
        boot:   Instant::now(),
    };
    slint::platform::set_platform(Box::new(platform))
        .expect("set_platform called twice");

    // 4) Build the UI tree. Must happen *after* set_platform.
    let app = AppWindow::new().expect("AppWindow::new");

    // Seed the param mirrors with the Slint defaults so the logger and the UI
    // agree on the very first tick.
    G_GAIN.store(app.get_gain().to_bits(), Ordering::Relaxed);
    G_DELAY.store(app.get_delay().to_bits(), Ordering::Relaxed);
    G_REVERB.store(app.get_reverb().to_bits(), Ordering::Relaxed);
    G_BYPASS.store(app.get_bypass(), Ordering::Relaxed);

    app.on_param_changed(|name, value| {
        let bits = value.to_bits();
        match name.as_str() {
            "gain"   => G_GAIN.store(bits, Ordering::Relaxed),
            "delay"  => G_DELAY.store(bits, Ordering::Relaxed),
            "reverb" => G_REVERB.store(bits, Ordering::Relaxed),
            _ => {}
        }
    });
    app.on_bypass_changed(|b| {
        G_BYPASS.store(b, Ordering::Relaxed);
    });

    // 5) Display + line buffer for the renderer. Wrapped in a RefCell so the
    //    closure passed to draw_if_needed can borrow it exclusively.
    let line_buf = RefCell::new(LineBuf {
        display: Box::leak(Box::new(Display { _spi: DummySpi })),
        line:    [Rgb565Pixel(0); DISPLAY_W as usize],
    });

    spawner.spawn(dsp_logger()).expect("spawn dsp_logger");

    // 6) Super-loop render. We deliberately do NOT override run_event_loop;
    //    the spec is to drive timers + draw_if_needed manually.
    loop {
        slint::platform::update_timers_and_animations();

        window.draw_if_needed(|renderer| {
            let mut lb = line_buf.borrow_mut();
            renderer.render_by_line(&mut *lb);
        });

        // Cooperative sleep until the next frame slot. A real build would also
        // wake on input-event interrupts; embassy-time is enough for the demo.
        Timer::after(Duration::from_millis(16)).await;
    }
}
