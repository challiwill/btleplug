// btleplug Source Code File
//
// Copyright 2020 Nonpolynomial Labs LLC. All rights reserved.
//
// Licensed under the BSD 3-Clause license. See LICENSE file in the project root
// for full license information.
//
// Some portions of this file are taken and/or modified from Rumble
// (https://github.com/mwylde/rumble), using a dual MIT/Apache License under the
// following copyright:
//
// Copyright (c) 2014 The Rust Project Developers

use uuid::Uuid;
use super::{ble::watcher::BLEWatcher, peripheral::Peripheral, peripheral::PeripheralId};
use crate::{
    api::{BDAddr, Central, CentralEvent, ScanFilter},
    common::adapter_manager::AdapterManager,
    Error, Result,
};
use async_trait::async_trait;
use futures::stream::Stream;
use std::convert::TryInto;
use std::fmt::{self, Debug, Formatter};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use windows::Devices::Bluetooth::BluetoothLEDevice;
use windows::Devices::Enumeration::DeviceInformation;

/// Implementation of [api::Central](crate::api::Central).
#[derive(Clone)]
pub struct Adapter {
    watcher: Arc<Mutex<BLEWatcher>>,
    manager: Arc<AdapterManager<Peripheral>>,
}

impl Adapter {
    pub(crate) fn new() -> Self {
        let watcher = Arc::new(Mutex::new(BLEWatcher::new()));
        let manager = Arc::new(AdapterManager::default());
        Adapter { watcher, manager }
    }
}

impl Debug for Adapter {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("Adapter")
            .field("manager", &self.manager)
            .finish()
    }
}

#[async_trait]
impl Central for Adapter {
    type Peripheral = Peripheral;

    async fn events(&self) -> Result<Pin<Box<dyn Stream<Item = CentralEvent> + Send>>> {
        Ok(self.manager.event_stream())
    }

    async fn connected_peripherals(&self, filter: ScanFilter) -> Result<()> {
        /* TODO unwrap is unsafe. */
        /* TODO filter for MouthPad and return that. */
        let service_filter = filter.services[0];
        let devices = match DeviceInformation::FindAllAsyncAqsFilter(
            &BluetoothLEDevice::GetDeviceSelector().unwrap(),
        )
        .unwrap()
        .get()
        {
            Ok(devices) => devices,
            Err(e) => {
                return Err(Error::Other(format!("{:?}", e).into()));
            }
        };
        let manager = self.manager.clone();

        for device in devices {
            let device_id = device.Id().unwrap();
            println!("Device ID: {:?}", device_id);
            let ble_device = match BluetoothLEDevice::FromIdAsync(&device_id) {
                Ok(ble_device) => ble_device,
                Err(e) => {
                    println!("Error getting ble device from id: {:?}", e);
                    continue;
                }
            };
            let ble_device = match ble_device.get() {
                Ok(ble_device) => ble_device,
                Err(e) => {
                    println!("Error getting ble device: {:?}", e);
                    continue;
                }
            };
            let services = ble_device
                .GetGattServicesAsync()
                .unwrap()
                .get()
                .unwrap()
                .Services()
                .unwrap();
            println!("got services");
            for service in services {
                println!("Service: {:?}", service.Uuid().unwrap());
                let service_uuid = Uuid::from_u128(service.Uuid().unwrap().to_u128());
                if service_uuid == service_filter {
                    let bluetooth_address = ble_device.BluetoothAddress().unwrap();
                    let address: BDAddr = bluetooth_address.try_into().unwrap();
                    let peripheral = Peripheral::new(Arc::downgrade(&manager), address);
                    // TODO this populates things like the device name
                    // peripheral.update_properties(args);
                    manager.add_peripheral(peripheral);
                    manager.emit(CentralEvent::DeviceDiscovered(address.into()));
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    async fn start_scan(&self, filter: ScanFilter) -> Result<()> {
        let watcher = self.watcher.lock().unwrap();
        let manager = self.manager.clone();
        watcher.start(
            filter,
            Box::new(move |args| {
                let bluetooth_address = args.BluetoothAddress().unwrap();
                let address: BDAddr = bluetooth_address.try_into().unwrap();
                if let Some(mut entry) = manager.peripheral_mut(&address.into()) {
                    entry.value_mut().update_properties(args);
                    manager.emit(CentralEvent::DeviceUpdated(address.into()));
                } else {
                    let peripheral = Peripheral::new(Arc::downgrade(&manager), address);
                    peripheral.update_properties(args);
                    manager.add_peripheral(peripheral);
                    manager.emit(CentralEvent::DeviceDiscovered(address.into()));
                }
            }),
        )
    }

    async fn stop_scan(&self) -> Result<()> {
        let watcher = self.watcher.lock().unwrap();
        watcher.stop().unwrap();
        Ok(())
    }

    async fn peripherals(&self) -> Result<Vec<Peripheral>> {
        Ok(self.manager.peripherals())
    }

    async fn peripheral(&self, id: &PeripheralId) -> Result<Peripheral> {
        self.manager.peripheral(id).ok_or(Error::DeviceNotFound)
    }

    async fn add_peripheral(&self, _address: &PeripheralId) -> Result<Peripheral> {
        Err(Error::NotSupported(
            "Can't add a Peripheral from a BDAddr".to_string(),
        ))
    }

    async fn adapter_info(&self) -> Result<String> {
        // TODO: Get information about the adapter.
        Ok("WinRT".to_string())
    }
}
