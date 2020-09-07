#![no_std]
#![no_main]

extern crate cortex_m_rt as rt;
extern crate ebyte_e73_tbx_bsp;
extern crate panic_semihosting;

use cortex_m_rt::entry;

use ebyte_e73_tbx_bsp::{
    hal::gpio::Level,
    hal::spim::{Frequency, Mode, Phase, Pins, Polarity, Spim},
    hal::Delay,
    Board,
};

use ili9341::*;

use embedded_graphics::{
    image::{Image, ImageRaw},
    pixelcolor::{raw::LittleEndian, Rgb565},
    prelude::*,
};

use jpegdec_sys::*;

// Source JPEG
const PRIDE_CONST: &[u8; 5184] = include_bytes!("../assets/rust-pride.jpg");
// Uncompressed version of our jpeg
static mut IMGBUF: [u16; 4096] = [0; 4096];

/// Callback function for passing to JPEGDEC that will handle drawing
/// Assuming for now we're feeding into a buffer exactly the size of the image
extern "C" fn callback(p_draw: *mut JPEGDRAW) {
    let data = unsafe { *p_draw };
    let start_x = data.x as usize;
    let start_y = data.y as usize;
    let draw_width = data.iWidth as usize;
    let draw_height = data.iHeight as usize;
    let pixel_data = data.pPixels;
    // TODO: verify BPP, conditionally use different conversion function
    let _bpp = data.iBpp;

    for y in 0..draw_height {
        // Since we're using byte indexes into single dimension objects for display, we need to calcuate
        // how far through we are for each x/y position. This is going to depend on the width of the
        // buffer for both the source (JPEG) and destination (scratch buffer)
        let src_y_offset = y * draw_width;
        let dst_y_offset = (y + start_y) * draw_width;

        for x in 0..draw_width {
            let src_offset = x + src_y_offset;
            let dst_offset = x + dst_y_offset + start_x;
            unsafe {
                // In case we've messed up our raw buffer size, don't allow us to index past the end
                if dst_offset > IMGBUF.len() {
                    return;
                }
                IMGBUF[dst_offset] = *pixel_data.add(src_offset);
            }
        }
    }
}

#[entry]
fn main() -> ! {
    let mut board = Board::take().unwrap();

    // Physical pin mapping for ili9341 - change to match your wiring!
    // cs 25
    // reset 26
    // a0/dc (data/command) 27
    // sda 28
    // sck 29
    let spi_peripheral = board.SPIM0;
    let spi_pins = Pins {
        sck: board.pins.P0_29.into_push_pull_output(Level::Low).degrade(),
        mosi: Some(board.pins.P0_28.into_push_pull_output(Level::Low).degrade()),
        miso: None,
    };
    let spi_freq: Frequency = Frequency::M8;
    let spi_mode = Mode {
        polarity: Polarity::IdleLow,
        phase: Phase::CaptureOnFirstTransition,
    };
    let spi_over_read_character = 0;

    let lcd_spi_peripheral = Spim::new(
        spi_peripheral,
        spi_pins,
        spi_freq,
        spi_mode,
        spi_over_read_character,
    );
    let dc_pin = board.pins.P0_27.into_push_pull_output(Level::Low);
    let cs_pin = board.pins.P0_25.into_push_pull_output(Level::Low);

    let lcd_interface =
        display_interface_spi::SPIInterface::new(lcd_spi_peripheral, dc_pin, cs_pin);
    let reset_pin = board.pins.P0_26.into_push_pull_output(Level::Low);
    let mut delay = Delay::new(board.SYST);

    let mut lcd = Ili9341::new(lcd_interface, reset_pin, &mut delay).unwrap();
    lcd.set_orientation(Orientation::Landscape).unwrap();

    // Width and height are hard-coded in the Ili9341 library, and the hardware won't tell us, so this size is wrong
    //let screen = lcd.size();
    // Instead, put your screen dimensions here, if you're going to use coordinates
    let screen = Size::new(128, 128);

    let mut image = unsafe { JPEG_ZeroInitJPEGIMAGE() };
    let imgptr: *mut JPEGIMAGE = &mut image as *mut JPEGIMAGE;

    let mut led_is_on = false;
    let mut rng = oorandom::Rand32::new(0);

    loop {
        // This doesn't have to happen in the loop since we're decoding the same image each time
        // but to prepare for more decoding we'll be honest.
        let opened = unsafe {
            JPEG_openRAM(
                imgptr,
                PRIDE_CONST.as_ptr(),
                PRIDE_CONST.len() as i32,
                Some(callback),
            )
        };
        if opened != 0 {
            let rc = unsafe { JPEG_decode(imgptr, 0, 0, 0) };
            if rc != 0 {
                // TODO: add time accounting
                // let elapsed = SystemTime::now().duration_since(start).unwrap().as_micros();
                // println!("full size decode in {} us", elapsed);
            }
            unsafe {
                JPEG_close(imgptr);
            }
        } else {
            // let errstr = unsafe { JPEG_getLastError(imgptr)};
            // println!("Last error: {}", errstr);
        }

        let imgbuf_u8slice = unsafe { core::mem::transmute::<[u16; 4096], [u8; 8192]>(IMGBUF) };
        let img_sz = Size::new(64, 64);
        let image2: ImageRaw<Rgb565, LittleEndian> =
            ImageRaw::new(&imgbuf_u8slice, img_sz.width, img_sz.height);

        let target_point = Point::new(
            rng.rand_range(0..(screen.width - img_sz.width)) as i32,
            rng.rand_range(0..(screen.height - img_sz.height)) as i32,
        );
        let img: Image<_, Rgb565> = Image::new(&image2, target_point);
        img.draw(&mut lcd).unwrap();

        if led_is_on {
            board.leds.led_1.disable();
        } else {
            board.leds.led_1.enable();
        }
        led_is_on = !led_is_on;
    }
}
