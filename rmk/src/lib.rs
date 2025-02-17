#![doc = include_str!("../README.md")]
//! ## Feature flags
#![doc = document_features::document_features!()]
// Make compiler and rust analyzer happy
#![allow(dead_code)]
#![allow(non_snake_case, non_upper_case_globals)]
// Enable std for espidf and test
#![cfg_attr(not(test), no_std)]
#![cfg_attr(no_std, not(target_os = "espidf"))]

#[cfg(feature = "_esp_ble")]
pub use crate::ble::esp::initialize_esp_ble_keyboard_with_config_and_run;
#[cfg(feature = "_nrf_ble")]
pub use crate::ble::nrf::initialize_nrf_ble_keyboard_with_config_and_run;
use crate::{
    keyboard::keyboard_task,
    light::{led_task, LightService},
    via::vial_task,
};
use action::KeyAction;
use core::cell::RefCell;
use defmt::*;
use embassy_futures::select::{select, select4, Either, Either4};
use embassy_time::Timer;
use embassy_usb::driver::Driver;
pub use embedded_hal;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_storage::nor_flash::NorFlash;
use embedded_storage_async::nor_flash::NorFlash as AsyncNorFlash;
use futures::pin_mut;
use keyboard::Keyboard;
use keymap::KeyMap;
use rmk_config::{RmkConfig, VialConfig};
use storage::Storage;
use usb::KeyboardUsbDevice;
use via::process::VialService;
pub use rmk_config as config;

pub mod action;
#[cfg(feature = "_ble")]
pub mod ble;
mod debounce;
mod flash;
mod hid;
pub mod keyboard;
pub mod keycode;
pub mod keymap;
pub mod layout_macro;
mod light;
mod matrix;
mod storage;
mod usb;
mod via;

/// Initialize and run the keyboard service, this function never returns.
///
/// # Arguments
///
/// * `driver` - embassy usb driver instance
/// * `input_pins` - input gpio pins
/// * `output_pins` - output gpio pins
/// * `flash` - optional flash storage, which is used for storing keymap and keyboard configs
/// * `keymap` - default keymap definition
/// * `vial_keyboard_id`/`vial_keyboard_def` - generated keyboard id and definition for vial, you can generate them automatically using [`build.rs`](https://github.com/HaoboGu/rmk/blob/main/boards/stm32h7/build.rs)
pub async fn initialize_keyboard_and_run<
    D: Driver<'static>,
    In: InputPin,
    Out: OutputPin,
    F: NorFlash,
    const ROW: usize,
    const COL: usize,
    const NUM_LAYER: usize,
>(
    driver: D,
    #[cfg(feature = "col2row")] input_pins: [In; ROW],
    #[cfg(not(feature = "col2row"))] input_pins: [In; COL],
    #[cfg(feature = "col2row")] output_pins: [Out; COL],
    #[cfg(not(feature = "col2row"))] output_pins: [Out; ROW],
    flash: Option<F>,
    keymap: [[[KeyAction; COL]; ROW]; NUM_LAYER],
    vial_keyboard_id: &'static [u8],
    vial_keyboard_def: &'static [u8],
) -> ! {
    let keyboard_config = RmkConfig {
        vial_config: VialConfig::new(vial_keyboard_id, vial_keyboard_def),
        ..Default::default()
    };

    initialize_keyboard_with_config_and_run(
        driver,
        input_pins,
        output_pins,
        flash,
        keymap,
        keyboard_config,
    )
    .await
}

/// Initialize and run the keyboard service, with given keyboard usb config. This function never returns.
///
/// # Arguments
///
/// * `driver` - embassy usb driver instance
/// * `input_pins` - input gpio pins
/// * `output_pins` - output gpio pins
/// * `flash` - optional flash storage, which is used for storing keymap and keyboard configs
/// * `keymap` - default keymap definition
/// * `keyboard_config` - other configurations of the keyboard, check [RmkConfig] struct for details
pub async fn initialize_keyboard_with_config_and_run<
    F: NorFlash,
    D: Driver<'static>,
    In: InputPin,
    Out: OutputPin,
    const ROW: usize,
    const COL: usize,
    const NUM_LAYER: usize,
>(
    driver: D,
    #[cfg(feature = "col2row")] input_pins: [In; ROW],
    #[cfg(not(feature = "col2row"))] input_pins: [In; COL],
    #[cfg(feature = "col2row")] output_pins: [Out; COL],
    #[cfg(not(feature = "col2row"))] output_pins: [Out; ROW],
    flash: Option<F>,
    keymap: [[[KeyAction; COL]; ROW]; NUM_LAYER],
    keyboard_config: RmkConfig<'static, Out>,
) -> ! {
    // Wrap `embedded-storage` to `embedded-storage-async`
    let async_flash = flash.map(|f| embassy_embedded_hal::adapter::BlockingAsync::new(f));

    initialize_keyboard_with_config_and_run_async_flash(
        driver,
        input_pins,
        output_pins,
        async_flash,
        keymap,
        keyboard_config,
    )
    .await
}

/// Initialize and run the keyboard service, with given keyboard usb config. This function never returns.
///
/// # Arguments
///
/// * `driver` - embassy usb driver instance
/// * `input_pins` - input gpio pins
/// * `output_pins` - output gpio pins
/// * `flash` - optional **async** flash storage, which is used for storing keymap and keyboard configs
/// * `keymap` - default keymap definition
/// * `keyboard_config` - other configurations of the keyboard, check [RmkConfig] struct for details
pub async fn initialize_keyboard_with_config_and_run_async_flash<
    F: AsyncNorFlash,
    D: Driver<'static>,
    In: InputPin,
    Out: OutputPin,
    const ROW: usize,
    const COL: usize,
    const NUM_LAYER: usize,
>(
    driver: D,
    #[cfg(feature = "col2row")] input_pins: [In; ROW],
    #[cfg(not(feature = "col2row"))] input_pins: [In; COL],
    #[cfg(feature = "col2row")] output_pins: [Out; COL],
    #[cfg(not(feature = "col2row"))] output_pins: [Out; ROW],
    flash: Option<F>,
    keymap: [[[KeyAction; COL]; ROW]; NUM_LAYER],
    keyboard_config: RmkConfig<'static, Out>,
) -> ! {
    // Initialize storage and keymap
    let (mut storage, keymap) = match flash {
        Some(f) => {
            let mut s = Storage::new(f, &keymap, keyboard_config.storage_config).await;
            let keymap = RefCell::new(
                KeyMap::<ROW, COL, NUM_LAYER>::new_from_storage(keymap, Some(&mut s)).await,
            );
            (Some(s), keymap)
        }
        None => {
            let keymap = RefCell::new(
                KeyMap::<ROW, COL, NUM_LAYER>::new_from_storage::<F>(keymap, None).await,
            );
            (None, keymap)
        }
    };

    // Create keyboard services and devices
    let (mut keyboard, mut usb_device, mut vial_service, mut light_service) = (
        Keyboard::new(input_pins, output_pins, &keymap),
        KeyboardUsbDevice::new(driver, keyboard_config.usb_config),
        VialService::new(&keymap, keyboard_config.vial_config),
        LightService::from_config(keyboard_config.light_config),
    );

    loop {
        // Run all tasks, if one of them fails, wait 1 second and then restart
        if let Some(ref mut s) = storage {
            run_usb_keyboard(
                &mut usb_device,
                &mut keyboard,
                s,
                &mut light_service,
                &mut vial_service,
            )
            .await;
        } else {
            // Run 4 tasks: usb, keyboard, led, vial
            let usb_fut = usb_device.device.run();
            let keyboard_fut = keyboard_task(
                &mut keyboard,
                &mut usb_device.keyboard_hid_writer,
                &mut usb_device.other_hid_writer,
            );
            let led_reader_fut = led_task(&mut usb_device.keyboard_hid_reader, &mut light_service);
            let via_fut = vial_task(&mut usb_device.via_hid, &mut vial_service);
            pin_mut!(usb_fut);
            pin_mut!(keyboard_fut);
            pin_mut!(led_reader_fut);
            pin_mut!(via_fut);
            match select4(usb_fut, keyboard_fut, led_reader_fut, via_fut).await {
                Either4::First(_) => {
                    error!("Usb task is died");
                }
                Either4::Second(_) => error!("Keyboard task is died"),
                Either4::Third(_) => error!("Led task is died"),
                Either4::Fourth(_) => error!("Via task is died"),
            }
        }

        warn!("Detected failure, restarting keyboard sevice after 1 second");
        Timer::after_secs(1).await;
    }
}

// Run usb keyboard task for once
pub(crate) async fn run_usb_keyboard<
    'a,
    'b,
    D: Driver<'a>,
    F: AsyncNorFlash,
    In: InputPin,
    Out: OutputPin,
    const ROW: usize,
    const COL: usize,
    const NUM_LAYER: usize,
>(
    usb_device: &mut KeyboardUsbDevice<'a, D>,
    keyboard: &mut Keyboard<'b, In, Out, ROW, COL, NUM_LAYER>,
    storage: &mut Storage<F>,
    light_service: &mut LightService<Out>,
    vial_service: &mut VialService<'b, ROW, COL, NUM_LAYER>,
) {
    let usb_fut = usb_device.device.run();
    let keyboard_fut = keyboard_task(
        keyboard,
        &mut usb_device.keyboard_hid_writer,
        &mut usb_device.other_hid_writer,
    );
    let led_reader_fut = led_task(&mut usb_device.keyboard_hid_reader, light_service);
    let via_fut = vial_task(&mut usb_device.via_hid, vial_service);
    let storage_fut = storage.run::<ROW, COL, NUM_LAYER>();
    pin_mut!(usb_fut);
    pin_mut!(keyboard_fut);
    pin_mut!(led_reader_fut);
    pin_mut!(via_fut);
    pin_mut!(storage_fut);
    match select4(
        select(usb_fut, keyboard_fut),
        storage_fut,
        led_reader_fut,
        via_fut,
    )
    .await
    {
        Either4::First(e) => match e {
            Either::First(_) => error!("Usb task is died"),
            Either::Second(_) => error!("Keyboard task is died"),
        },
        Either4::Second(_) => error!("Storage task is died"),
        Either4::Third(_) => error!("Led task is died"),
        Either4::Fourth(_) => error!("Via task is died"),
    }
}
