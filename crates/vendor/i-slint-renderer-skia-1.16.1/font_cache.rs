// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

use clru::CLruCache;
use i_slint_common::sharedfontique::HashedBlob;
use i_slint_core::textlayout::sharedparley::{fontique, parley};
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;

const FONT_CACHE_CAPACITY: NonZeroUsize = NonZeroUsize::new(64).unwrap();

pub struct FontCache {
    font_mgr: skia_safe::FontMgr,
    // Use HashedBlob in key to keep strong reference to font data blob,
    // preventing eviction from fontique's shared cache (see commit 30a03cf).
    // The u64 is a hash of variation settings (0 for base typefaces).
    fonts: CLruCache<(HashedBlob, u32, u64), Option<skia_safe::Typeface>>,
}

impl Default for FontCache {
    fn default() -> Self {
        Self { font_mgr: skia_safe::FontMgr::new(), fonts: CLruCache::new(FONT_CACHE_CAPACITY) }
    }
}

impl FontCache {
    pub fn font_with_variations(
        &mut self,
        font: &parley::FontData,
        synthesis: &fontique::Synthesis,
    ) -> Option<skia_safe::Typeface> {
        let variation_settings = synthesis.variation_settings();

        let mut variations_hash = 0u64;
        if !variation_settings.is_empty() {
            let mut hasher = DefaultHasher::new();
            for &(tag, value) in variation_settings {
                tag.to_be_bytes().hash(&mut hasher);
                value.to_bits().hash(&mut hasher);
            }
            variations_hash = hasher.finish();
        }

        let key = (font.data.clone().into(), font.index, variations_hash);

        if let Some(cached) = self.fonts.get(&key) {
            return cached.clone();
        }

        let mut typeface = self.load_typeface_internal(font);

        if !variation_settings.is_empty() {
            typeface = typeface.and_then(|base| {
                let coords: Vec<skia_safe::font_arguments::variation_position::Coordinate> =
                    variation_settings
                        .iter()
                        .map(|&(tag, value)| {
                            skia_safe::font_arguments::variation_position::Coordinate {
                                axis: skia_safe::FourByteTag::new(u32::from_be_bytes(
                                    tag.to_be_bytes(),
                                )),
                                value,
                            }
                        })
                        .collect();
                let position =
                    skia_safe::font_arguments::VariationPosition { coordinates: &coords };
                let args = skia_safe::FontArguments::new().set_variation_design_position(position);
                base.clone_with_arguments(&args).or(Some(base))
            });
        }

        self.fonts.put(key, typeface.clone());
        typeface
    }

    fn load_typeface_internal(&self, font: &parley::FontData) -> Option<skia_safe::Typeface> {
        let typeface = self.font_mgr.new_from_data(
            font.data.as_ref(),
            if font.index > 0 { Some(font.index as _) } else { None },
        );

        // Due to  https://issues.skia.org/issues/310510989, fonts from true type collections
        // with an index > 0 fail to load on macOS. As a workaround, we manually extract the font from the
        // collection and load it as a single font.
        #[cfg(target_vendor = "apple")]
        if font.index > 0 && typeface.is_none() {
            if let Some(typeface) = read_fonts::CollectionRef::new(font.data.as_ref())
                .ok()
                .and_then(|ttc| ttc.get(font.index).ok())
                .map(|ttf| write_fonts::FontBuilder::new().copy_missing_tables(ttf).build())
                .and_then(|new_ttf| self.font_mgr.new_from_data(&new_ttf, None))
            {
                return Some(typeface);
            }
            // Second-chance workaround (QBZ): for some system collections BOTH
            // paths above fail — AppleSDGothicNeo.ttc packs every weight at
            // index > 0 and CoreText also rejects the rebuilt sfnt, which
            // blanked every Korean glyph run at non-regular weights. Resolve
            // the SAME face through CoreText by family name + OS/2 weight; the
            // match points at the same installed file, so the glyph ids parley
            // shaped with stay valid — enforced by the cmap spot checks (a
            // mismatching face is rejected rather than drawing wrong glyphs).
            if let Some(typeface) = self.typeface_via_family_match(font) {
                return Some(typeface);
            }
        }

        typeface
    }

    /// See `load_typeface_internal`: resolve a TTC face CoreText refuses to
    /// load from data by asking the system font manager for the same family
    /// at the face's weight, then verify glyph-id agreement via the cmap.
    #[cfg(target_vendor = "apple")]
    fn typeface_via_family_match(&self, font: &parley::FontData) -> Option<skia_safe::Typeface> {
        use read_fonts::TableProvider;

        let face = read_fonts::CollectionRef::new(font.data.as_ref())
            .ok()?
            .get(font.index)
            .ok()?;

        // Typographic family (name id 16) wins over the legacy family (id 1);
        // either resolves through CoreText, localized values included.
        let name = face.name().ok()?;
        let mut family = String::new();
        for rec in name.name_record() {
            let id = rec.name_id().to_u16();
            if id == 1 || id == 16 {
                if let Ok(entry) = rec.string(name.string_data()) {
                    if id == 16 || family.is_empty() {
                        family = entry.chars().collect();
                    }
                }
            }
        }
        if family.is_empty() {
            return None;
        }

        let weight = face.os2().ok().map(|os2| os2.us_weight_class()).unwrap_or(400);
        let style = skia_safe::FontStyle::new(
            skia_safe::font_style::Weight::from(weight as i32),
            skia_safe::font_style::Width::NORMAL,
            skia_safe::font_style::Slant::Upright,
        );
        let typeface = self.font_mgr.match_family_style(&family, style)?;

        // Glyph-id sanity: the matched face must map the same codepoints to
        // the same glyph ids as the face parley shaped with. One disagreement
        // disqualifies the match (blank beats wrong glyphs).
        let cmap = face.cmap().ok()?;
        let skia_font = skia_safe::Font::new(typeface.clone(), 16.0);
        let mut verified = 0usize;
        for cp in [0x41u32, 0x4E00, 0xAC00, 0x3042] {
            if let Some(gid) = cmap.map_codepoint(cp) {
                if u32::from(skia_font.unichar_to_glyph(cp as i32)) != gid.to_u32() {
                    return None;
                }
                verified += 1;
            }
        }
        (verified > 0).then_some(typeface)
    }
}

thread_local! {
    pub static FONT_CACHE: RefCell<FontCache> = RefCell::new(Default::default())
}
