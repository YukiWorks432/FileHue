//! 拡張子とサムネイル提供元の識別子。

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Format {
    Ai,
    Eps,
    Svg,
    Psd,
    Psb,
    Pdf,
}

impl Format {
    pub const ALL: [Self; 6] = [
        Self::Ai,
        Self::Eps,
        Self::Svg,
        Self::Psd,
        Self::Psb,
        Self::Pdf,
    ];

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Ai => ".ai",
            Self::Eps => ".eps",
            Self::Svg => ".svg",
            Self::Psd => ".psd",
            Self::Psb => ".psb",
            Self::Pdf => ".pdf",
        }
    }

    pub const fn clsid(self) -> u128 {
        match self {
            Self::Ai => 0x3044d562_79ba_450f_8846_0e3a75f4f27d,
            Self::Eps => 0xbd42852c_4cb6_49a7_86ef_613becfd07ad,
            Self::Svg => 0xc62d2ac2_0fd5_4ec7_bb81_fd245220dda3,
            Self::Psd => 0x9a985903_78cf_4ee9_a896_df82c6203d13,
            Self::Psb => 0xeb15e8c2_d95f_409c_bad4_8bd323a79c89,
            Self::Pdf => 0x6bcacc0c_2741_443f_84e4_8ace88452cd5,
        }
    }

    pub fn from_clsid(value: u128) -> Option<Self> {
        Self::ALL.into_iter().find(|format| format.clsid() == value)
    }
}
