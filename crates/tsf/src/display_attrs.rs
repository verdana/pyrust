use windows::Win32::Foundation::COLORREF;
use windows::core::{implement, Error, Result as WinResult, GUID, HRESULT};
use windows::Win32::UI::TextServices::{
    IEnumTfDisplayAttributeInfo, IEnumTfDisplayAttributeInfo_Impl, ITfDisplayAttributeInfo,
    ITfDisplayAttributeInfo_Impl, TF_CT_COLORREF, TF_DA_ATTR_INFO, TF_DA_COLOR, TF_DA_COLOR_0,
    TF_DA_LINESTYLE, TF_DISPLAYATTRIBUTE,
};

/// Display attribute GUIDs — unique identifiers for each attribute type.
pub const GUID_ATTR_INPUT: GUID = GUID::from_u128(0xA1B2C3D4_1234_5678_9ABC_DEF012345678);
pub const GUID_ATTR_CONVERTED: GUID = GUID::from_u128(0xA1B2C3D4_1234_5678_9ABC_DEF012345679);

/// Display attribute for pinyin input (underlined text, e.g., "nihao").
#[implement(ITfDisplayAttributeInfo)]
pub struct InputDisplayAttr;

impl ITfDisplayAttributeInfo_Impl for InputDisplayAttr_Impl {
    fn GetGUID(&self) -> WinResult<GUID> {
        Ok(GUID_ATTR_INPUT)
    }

    fn GetDescription(&self) -> WinResult<windows::core::BSTR> {
        Ok(windows::core::BSTR::from("pyrust Input"))
    }

    fn GetAttributeInfo(&self, info: *mut TF_DISPLAYATTRIBUTE) -> WinResult<()> {
        unsafe {
            *info = TF_DISPLAYATTRIBUTE {
                crText: TF_DA_COLOR::default(),
                crBk: TF_DA_COLOR::default(),
                lsStyle: TF_DA_LINESTYLE(2), // TF_LS_DOT
                fBoldLine: false.into(),
                crLine: TF_DA_COLOR {
                    r#type: TF_CT_COLORREF,
                    Anonymous: TF_DA_COLOR_0 { cr: COLORREF(0) }, // black
                },
                bAttr: TF_DA_ATTR_INFO(0), // TF_ATTR_INPUT
            };
        }
        Ok(())
    }

    fn SetAttributeInfo(&self, _ptda: *const TF_DISPLAYATTRIBUTE) -> WinResult<()> {
        Err(Error::new(HRESULT(0x80004001u32 as i32), "Read-only"))
    }

    fn Reset(&self) -> WinResult<()> {
        Ok(())
    }
}

/// Display attribute for converted text (candidate selected, e.g., "你好").
#[implement(ITfDisplayAttributeInfo)]
pub struct ConvertedDisplayAttr;

impl ITfDisplayAttributeInfo_Impl for ConvertedDisplayAttr_Impl {
    fn GetGUID(&self) -> WinResult<GUID> {
        Ok(GUID_ATTR_CONVERTED)
    }

    fn GetDescription(&self) -> WinResult<windows::core::BSTR> {
        Ok(windows::core::BSTR::from("pyrust Converted"))
    }

    fn GetAttributeInfo(&self, info: *mut TF_DISPLAYATTRIBUTE) -> WinResult<()> {
        // SAFETY: info is a valid output pointer provided by TSF.
        unsafe {
            *info = TF_DISPLAYATTRIBUTE {
                crText: TF_DA_COLOR::default(),
                crBk: TF_DA_COLOR::default(),
                lsStyle: TF_DA_LINESTYLE(2), // TF_LS_DOT
                fBoldLine: false.into(),
                crLine: TF_DA_COLOR {
                    r#type: TF_CT_COLORREF,
                    Anonymous: TF_DA_COLOR_0 { cr: COLORREF(0) }, // black
                },
                bAttr: TF_DA_ATTR_INFO(1), // TF_ATTR_TARGET_CONVERTED
            };
        }
        Ok(())
    }

    fn SetAttributeInfo(&self, _ptda: *const TF_DISPLAYATTRIBUTE) -> WinResult<()> {
        Err(Error::new(HRESULT(0x80004001u32 as i32), "Read-only"))
    }

    fn Reset(&self) -> WinResult<()> {
        Ok(())
    }
}

/// Enumerator that yields the two display attribute infos.
#[implement(IEnumTfDisplayAttributeInfo)]
pub struct PyrustEnumDisplayAttr {
    index: std::sync::atomic::AtomicU32,
}

impl PyrustEnumDisplayAttr {
    pub fn new() -> Self {
        Self {
            index: std::sync::atomic::AtomicU32::new(0),
        }
    }
}

impl IEnumTfDisplayAttributeInfo_Impl for PyrustEnumDisplayAttr_Impl {
    fn Clone(&self) -> WinResult<IEnumTfDisplayAttributeInfo> {
        let cloned = PyrustEnumDisplayAttr::new();
        cloned.index.store(
            self.index.load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
        Ok(cloned.into())
    }

    fn Next(
        &self,
        ulcount: u32,
        rginfo: *mut Option<ITfDisplayAttributeInfo>,
        pcfetched: *mut u32,
    ) -> WinResult<()> {
        let current = self.index.load(std::sync::atomic::Ordering::Relaxed);
        let mut fetched: u32 = 0;

        // SAFETY: rginfo is a valid array of size ulcount, caller guarantees this.
        unsafe {
            for i in 0..ulcount {
                let attr: Option<ITfDisplayAttributeInfo> = match current + i {
                    0 => Some(InputDisplayAttr.into()),
                    1 => Some(ConvertedDisplayAttr.into()),
                    _ => None,
                };
                if attr.is_some() {
                    *rginfo.add(i as usize) = attr;
                    fetched += 1;
                } else {
                    break;
                }
            }
            if !pcfetched.is_null() {
                *pcfetched = fetched;
            }
        }

        self.index
            .store(current + fetched, std::sync::atomic::Ordering::Relaxed);

        if fetched < ulcount {
            Err(Error::new(
                HRESULT(0x00000001u32 as i32), // S_FALSE = no more items
                "End of enumeration",
            ))
        } else {
            Ok(())
        }
    }

    fn Reset(&self) -> WinResult<()> {
        self.index
            .store(0, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    fn Skip(&self, ulcount: u32) -> WinResult<()> {
        let current = self.index.load(std::sync::atomic::Ordering::Relaxed);
        let new_index = current + ulcount;
        if new_index > 2 {
            self.index
                .store(2, std::sync::atomic::Ordering::Relaxed);
            Err(Error::new(
                HRESULT(0x00000001u32 as i32), // S_FALSE
                "Skipped past end",
            ))
        } else {
            self.index
                .store(new_index, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        }
    }
}
