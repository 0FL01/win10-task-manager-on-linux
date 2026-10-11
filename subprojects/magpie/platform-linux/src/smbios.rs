use smbioslib::{table_load_from_device, DefinedStruct, PortableBatteryDesignCapacity};

pub fn design_capacity_mwh() -> Option<u32> {
    let data = table_load_from_device().ok()?;

    for raw in data.iter() {
        if let DefinedStruct::PortableBattery(battery) = DefinedStruct::from(raw) {
            let capacity = match battery.design_capacity()? {
                PortableBatteryDesignCapacity::MilliWattHours(v) => v as u32,
                PortableBatteryDesignCapacity::Unknown => continue,
            };

            let multiplier = battery.design_capacity_multiplier().unwrap_or(1) as u32;

            return Some(capacity * multiplier);
        }
    }

    None
}
