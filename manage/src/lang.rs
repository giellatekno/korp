//! The languages (Korp instances) we know about.

macro_rules! langs {
    ($($variant:ident => $code:literal,)*) => {
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        pub enum Lang {
            $($variant,)*
        }

        impl Lang {
            pub const ALL: &[Lang] = &[$(Lang::$variant,)*];

            pub fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $code,)*
                }
            }
        }
    };
}

langs! {
    Fao => "fao",
    Fit => "fit",
    Fkv => "fkv",
    Koi => "koi",
    Kpv => "kpv",
    Mdf => "mdf",
    Mhr => "mhr",
    Mrj => "mrj",
    Myv => "myv",
    Olo => "olo",
    Sma => "sma",
    Sme => "sme",
    Smj => "smj",
    Smn => "smn",
    Sms => "sms",
    Udm => "udm",
    Vep => "vep",
    Vro => "vro",
}

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Lang {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|lang| lang.as_str() == s)
            .ok_or_else(|| {
                let all: Vec<&str> = Self::ALL.iter().map(Lang::as_str).collect();
                format!("unknown language '{s}' (known: {})", all.join(", "))
            })
    }
}
