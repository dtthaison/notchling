use icu_decimal::input::Decimal;
use icu_decimal::{DecimalFormatter, DecimalFormatterPreferences};
use icu_locale_core::subtags::Region;
use icu_locale_core::Locale;
use icu_plurals::{PluralCategory, PluralRules, PluralRulesPreferences};

use crate::format::{Cat, Numbers};

pub(crate) struct Icu {
    plurals: Option<PluralRules>,
    decimal: Option<DecimalFormatter>,
}

fn locale_of(tag: &str, region: Option<&str>) -> Locale {
    let mut locale = Locale::try_from_str(tag)
        .or_else(|_| Locale::try_from_str("en"))
        .unwrap_or(Locale::UNKNOWN);
    if let Some(region) = region.and_then(|r| Region::try_from_str(r).ok()) {
        locale.id.region = Some(region);
    }
    locale
}

impl Icu {
    pub(crate) fn new(tag: &str, region: Option<&str>) -> Icu {
        let locale = locale_of(tag, region);
        let plurals = PluralRules::try_new_cardinal(PluralRulesPreferences::from(&locale)).ok();
        let decimal = DecimalFormatter::try_new(
            DecimalFormatterPreferences::from(&locale),
            Default::default(),
        )
        .ok();
        Icu { plurals, decimal }
    }

    pub(crate) fn decimal(&self, value: i64, shift: i16) -> String {
        let mut number = Decimal::from(value);
        number.multiply_pow10(shift);
        match &self.decimal {
            Some(formatter) => formatter.format(&number).to_string(),
            None => number.to_string(),
        }
    }
}

impl Numbers for Icu {
    fn int(&self, n: i64) -> String {
        self.decimal(n, 0)
    }

    fn category(&self, n: i64) -> Cat {
        match &self.plurals {
            Some(rules) => match rules.category_for(n) {
                PluralCategory::Zero => Cat::Zero,
                PluralCategory::One => Cat::One,
                PluralCategory::Two => Cat::Two,
                PluralCategory::Few => Cat::Few,
                PluralCategory::Many => Cat::Many,
                PluralCategory::Other => Cat::Other,
            },
            None => {
                if n == 1 {
                    Cat::One
                } else {
                    Cat::Other
                }
            }
        }
    }
}
