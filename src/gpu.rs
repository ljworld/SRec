//! Graphics card + display detection through DXGI.

use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

#[derive(Clone, Debug)]
pub struct Adapter {
    pub name: String,
    pub vendor: Vendor,
    pub vram_mb: u64,
}

/// A monitor attached to the primary adapter (the adapter `ddagrab` uses).
/// Coordinates are physical pixels on the virtual desktop.
#[derive(Clone, Debug)]
pub struct Display {
    pub index: u32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            index: 0,
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        }
    }
}

impl Display {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

fn wstr(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n]).trim().to_string()
}

pub fn detect() -> (Vec<Adapter>, Vec<Display>) {
    let mut adapters = Vec::new();
    let mut displays = Vec::new();
    let factory: IDXGIFactory1 = match unsafe { CreateDXGIFactory1() } {
        Ok(f) => f,
        Err(_) => return (adapters, displays),
    };
    let mut i = 0u32;
    loop {
        let adapter = match unsafe { factory.EnumAdapters1(i) } {
            Ok(a) => a,
            Err(_) => break,
        };
        let idx = i;
        i += 1;

        if idx == 0 {
            let mut o = 0u32;
            while let Ok(out) = unsafe { adapter.EnumOutputs(o) } {
                if let Ok(d) = unsafe { out.GetDesc() } {
                    let r = d.DesktopCoordinates;
                    displays.push(Display {
                        index: o,
                        x: r.left,
                        y: r.top,
                        w: r.right - r.left,
                        h: r.bottom - r.top,
                    });
                }
                o += 1;
            }
        }

        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        if desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
            continue;
        }
        let vendor = match desc.VendorId {
            0x10DE => Vendor::Nvidia,
            0x1002 | 0x1022 => Vendor::Amd,
            0x8086 => Vendor::Intel,
            0x1414 => continue, // Microsoft Basic Render Driver
            _ => Vendor::Other,
        };
        adapters.push(Adapter {
            name: wstr(&desc.Description),
            vendor,
            vram_mb: desc.DedicatedVideoMemory as u64 / (1024 * 1024),
        });
    }
    (adapters, displays)
}
