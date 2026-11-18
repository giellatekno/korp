/// An instance specifier is a string on the form "front-sme", i.e. "front" or "back",
/// a "-", and then a language.

#[derive(Copy, Clone)]
pub enum Lang {
    Fao,
    Fit,
    Fkv,
    Koi,
    Kpv,
    Mdf,
    Mhr,
    Mrj,
    Myv,
    Olo,
    Sma,
    Sme,
    Smj,
    Smn,
    Sms,
    Udm,
    Vep,
    Vro,
}

impl Lang {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Fao => "fao",
            Self::Fit => "fit",
            Self::Fkv => "fkv",
            Self::Koi => "koi",
            Self::Kpv => "kpv",
            Self::Mdf => "mdf",
            Self::Mhr => "mhr",
            Self::Mrj => "mrj",
            Self::Myv => "myv",
            Self::Olo => "olo",
            Self::Sma => "sma",
            Self::Sme => "sme",
            Self::Smj => "smj",
            Self::Smn => "smn",
            Self::Sms => "sms",
            Self::Udm => "udm",
            Self::Vep => "vep",
            Self::Vro => "vro",
        }
    }
}

impl std::str::FromStr for Lang {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "fao" => Ok(Self::Fao),
            "fit" => Ok(Self::Fit),
            "fkv" => Ok(Self::Fkv),
            "koi" => Ok(Self::Koi),
            "kpv" => Ok(Self::Kpv),
            "mdf" => Ok(Self::Mdf),
            "mhr" => Ok(Self::Mhr),
            "mrj" => Ok(Self::Mrj),
            "myv" => Ok(Self::Myv),
            "olo" => Ok(Self::Olo),
            "sma" => Ok(Self::Sma),
            "sme" => Ok(Self::Sme),
            "smj" => Ok(Self::Smj),
            "smn" => Ok(Self::Smn),
            "sms" => Ok(Self::Sms),
            "udm" => Ok(Self::Udm),
            "vep" => Ok(Self::Vep),
            "vro" => Ok(Self::Vro),
            other => Err(format!("unrecognized lang '{other}'")),
        }
    }
}

/// Which "side" (in lack of a better term): backend or frontend
#[derive(Copy, Clone)]
pub enum Side {
    Back,
    Front,
}

impl std::str::FromStr for Side {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "front" => Ok(Self::Front),
            "back" => Ok(Self::Back),
            other => Err(format!("'side' must be 'front' or 'back', not '{other}'")),
        }
    }
}

#[derive(Copy, Clone)]
pub struct Instance {
    pub side: Side,
    pub lang: Lang,
}

impl Instance {
    pub fn containerfile_path_str(&self) -> &str {
        match self.side {
            Side::Front => "containerfiles/Containerfile.frontend",
            Side::Back => panic!("containerfile not here"),
        }
    }

    pub fn is_backend(&self) -> bool {
        matches!(self.side, Side::Back)
    }

    pub fn image_name(&self) -> &str {
        match (self.side, self.lang) {
            (Side::Back, Lang::Fao) => "korp-backend-fao",
            (Side::Back, Lang::Fit) => "korp-backend-fit",
            (Side::Back, Lang::Fkv) => "korp-backend-fkv",
            (Side::Back, Lang::Koi) => "korp-backend-koi",
            (Side::Back, Lang::Kpv) => "korp-backend-kpv",
            (Side::Back, Lang::Mdf) => "korp-backend-mdf",
            (Side::Back, Lang::Mhr) => "korp-backend-mhr",
            (Side::Back, Lang::Mrj) => "korp-backend-mrj",
            (Side::Back, Lang::Myv) => "korp-backend-myv",
            (Side::Back, Lang::Olo) => "korp-backend-olo",
            (Side::Back, Lang::Sma) => "korp-backend-sma",
            (Side::Back, Lang::Sme) => "korp-backend-sme",
            (Side::Back, Lang::Smj) => "korp-backend-smj",
            (Side::Back, Lang::Smn) => "korp-backend-smn",
            (Side::Back, Lang::Sms) => "korp-backend-sms",
            (Side::Back, Lang::Udm) => "korp-backend-udm",
            (Side::Back, Lang::Vep) => "korp-backend-vep",
            (Side::Back, Lang::Vro) => "korp-backend-vro",
            (Side::Front, Lang::Fao) => "korp-frontend-fao",
            (Side::Front, Lang::Fit) => "korp-frontend-fit",
            (Side::Front, Lang::Fkv) => "korp-frontend-fkv",
            (Side::Front, Lang::Koi) => "korp-frontend-koi",
            (Side::Front, Lang::Kpv) => "korp-frontend-kpv",
            (Side::Front, Lang::Mdf) => "korp-frontend-mdf",
            (Side::Front, Lang::Mhr) => "korp-frontend-mhr",
            (Side::Front, Lang::Mrj) => "korp-frontend-mrj",
            (Side::Front, Lang::Myv) => "korp-frontend-myv",
            (Side::Front, Lang::Olo) => "korp-frontend-olo",
            (Side::Front, Lang::Sma) => "korp-frontend-sma",
            (Side::Front, Lang::Sme) => "korp-frontend-sme",
            (Side::Front, Lang::Smj) => "korp-frontend-smj",
            (Side::Front, Lang::Smn) => "korp-frontend-smn",
            (Side::Front, Lang::Sms) => "korp-frontend-sms",
            (Side::Front, Lang::Udm) => "korp-frontend-udm",
            (Side::Front, Lang::Vep) => "korp-frontend-vep",
            (Side::Front, Lang::Vro) => "korp-frontend-vro",
        }
    }
}

impl std::str::FromStr for Instance {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (side, lang) = s
            .split_once("-")
            .ok_or_else(|| String::from("instance must contain a '-'"))?;
        let side = Side::from_str(side)?;
        let lang = Lang::from_str(lang)?;
        Ok(Self { side, lang })
    }
}
