/* src/scaling.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

#[derive(Default, Clone, PartialEq)]
pub enum ScalingSettings {
    #[default]
    Fixed,
    ScaleUp,
    ScaleDown,
    ScaleUpDown,
    StickyUp,
    StickyDown,
    StickyUpDown,
    StickyUpDownEqualMagnitude,
    Stacking,
}

#[derive(Default, Clone, PartialEq)]
pub enum RoundingSettings {
    #[default]
    NoRounding,
    Pow2,
    Pow2Base10,
    Integer,
    Pow10,
}

impl RoundingSettings {
    fn round_up_to_next_power_of_two(num: f32) -> f32 {
        let num = num as u64;

        if num == 0 {
            return 0.;
        }

        let mut n = num - 1;
        n |= n >> 1;
        n |= n >> 2;
        n |= n >> 4;
        n |= n >> 8;
        n |= n >> 16;

        (n + 1) as f32
    }

    fn round_up_to_next_power_of_two_base_10(num: f32) -> f32 {
        if num == 0. {
            return 0.;
        }

        // take the power of two amount w.r.t. the last power of 1000
        let log1000 = (num.log10() / 3.) as i32;

        let num_below = 1000f32.powi(log1000);

        Self::round_up_to_next_power_of_two((num / num_below).ceil()).min(1000.) * num_below
    }

    fn round_up_to_next_power_of_ten(mut n: f32) -> f32 {
        let mut negative_multiplier = 1.;

        if n == 0. {
            return 0.;
        } else if n < 0. {
            negative_multiplier = -1.;
            n = n.abs()
        }

        let magnitude = 10_f32.powi(n.log10().floor() as i32);
        (n / magnitude).ceil() * magnitude * negative_multiplier
    }

    pub fn apply_up_rounding(&self, f: f32) -> f32 {
        match self {
            RoundingSettings::NoRounding => f,
            RoundingSettings::Pow2 => Self::round_up_to_next_power_of_two(f),
            RoundingSettings::Pow2Base10 => Self::round_up_to_next_power_of_two_base_10(f),
            RoundingSettings::Integer => f.ceil(),
            RoundingSettings::Pow10 => Self::round_up_to_next_power_of_ten(f),
        }
    }

    pub fn apply_down_rounding(&self, f: f32) -> f32 {
        match self {
            RoundingSettings::NoRounding => f,
            RoundingSettings::Integer => f.floor(),
            RoundingSettings::Pow10 => Self::round_up_to_next_power_of_ten(f),
            _ => f, // generally rounding down on logs is problematic
        }
    }
}

#[derive(Default, Clone, PartialEq)]
pub enum FillingSettings {
    #[default]
    FillToBottom,
    FillToTop,
    FillToZero,
    None,
}
