//! Decoder for the embedded country shapes (`MOSSCTY1`, written by
//! `scripts/place-map/generate-countries.mjs`).

use std::sync::OnceLock;

const MAGIC: &[u8; 8] = b"MOSSCTY1";
const SCHEMA: u16 = 1;
const PACKED: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/place-map/countries-v1.bin.br"));

/// One country as Natural Earth names it, with its outline in degrees.
#[derive(Debug, Clone)]
pub(crate) struct Country {
    /// ISO 3166 alpha-3 where Natural Earth has one, else its own admin code.
    pub code: String,
    pub adm0: String,
    pub name: String,
    pub name_long: String,
    pub admin: String,
    pub formal: String,
    /// Land that is disputed or breakaway throughout: a name without a shape.
    pub disputed_name: bool,
    pub rings: Vec<Vec<(f64, f64)>>,
}

#[derive(Debug)]
pub(crate) struct Shapes {
    /// The first 16 hex digits of the source manifest's SHA-256, naming the notice's directory.
    pub digest: String,
    pub countries: Vec<Country>,
    /// Disputed and breakaway land, drawn as plain land.
    pub disputed: Vec<Vec<(f64, f64)>>,
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> &[u8] {
        let slice = &self.bytes[self.at..self.at + n];
        self.at += n;
        slice
    }
    fn varint(&mut self) -> u64 {
        let (mut value, mut shift) = (0u64, 0);
        loop {
            let byte = self.bytes[self.at];
            self.at += 1;
            value |= u64::from(byte & 127) << shift;
            if byte < 128 {
                return value;
            }
            shift += 7;
        }
    }
    fn string(&mut self) -> String {
        let n = self.varint() as usize;
        String::from_utf8_lossy(self.take(n)).into_owned()
    }
    fn rings(&mut self, quant: f64) -> Vec<Vec<(f64, f64)>> {
        let (mut x, mut y) = (0i64, 0i64);
        let unzig = |v: u64| if v & 1 == 1 { -((v as i64 + 1) / 2) } else { (v / 2) as i64 };
        (0..self.varint())
            .map(|_| {
                (0..self.varint())
                    .map(|_| {
                        x += unzig(self.varint());
                        y += unzig(self.varint());
                        (x as f64 / quant, y as f64 / quant)
                    })
                    .collect()
            })
            .collect()
    }
}

fn decode(bytes: &[u8]) -> Shapes {
    assert_eq!(&bytes[..8], MAGIC, "country shapes: bad magic");
    assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), SCHEMA, "country shapes: unknown schema");
    let quant = f64::from(u16::from_le_bytes([bytes[10], bytes[11]]));
    let mut r = Reader { bytes, at: 12 + 32 };
    let countries = (0..r.varint())
        .map(|_| Country {
            code: r.string(),
            adm0: r.string(),
            name: r.string(),
            name_long: r.string(),
            admin: r.string(),
            formal: r.string(),
            disputed_name: r.take(1)[0] == 1,
            rings: r.rings(quant),
        })
        .collect();
    let disputed = r.rings(quant);
    let digest = bytes[12..28].iter().map(|b| format!("{b:02x}")).collect();
    Shapes { digest, countries, disputed }
}

/// The embedded shapes, decoded once.
pub(crate) fn embedded() -> &'static Shapes {
    static SHAPES: OnceLock<Shapes> = OnceLock::new();
    SHAPES.get_or_init(|| {
        let mut raw = Vec::new();
        brotli::BrotliDecompress(&mut std::io::Cursor::new(PACKED), &mut raw)
            .expect("embedded country shapes must be valid brotli");
        decode(&raw)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_embedded_shapes() {
        let shapes = embedded();
        assert!(shapes.countries.len() > 230);
        assert!(shapes.countries.iter().all(|c| c.name != "Antarctica"));
        let mexico = shapes.countries.iter().find(|c| c.code == "MEX").expect("Mexico");
        assert!(!mexico.disputed_name && !mexico.rings.is_empty());
        let (lon, lat) = mexico.rings[0][0];
        assert!((-120.0..-85.0).contains(&lon) && (13.0..34.0).contains(&lat), "{lon},{lat}");
        assert!(!shapes.disputed.is_empty());
    }

    #[test]
    fn france_and_norway_have_codes_although_their_iso_column_is_blank() {
        let codes: Vec<&str> = embedded().countries.iter().map(|c| c.code.as_str()).collect();
        for code in ["FRA", "NOR"] {
            assert!(codes.contains(&code), "{code} missing");
        }
        assert!(codes.iter().all(|c| *c != "-99"));
    }

    #[test]
    fn disputed_names_have_no_shape() {
        let somaliland = embedded().countries.iter().find(|c| c.name == "Somaliland").expect("Somaliland");
        assert!(somaliland.disputed_name && somaliland.rings.is_empty());
    }
}
