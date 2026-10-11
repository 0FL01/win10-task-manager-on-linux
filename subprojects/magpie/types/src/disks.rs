/* src/disks.rs
 *
 * Copyright 2025 Mission Center Developers
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

include!(concat!(env!("OUT_DIR"), "/magpie.disks.rs"));

use smart_data::TestResult;

impl<'a> TryFrom<&'a str> for TestResult {
    type Error = ();

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        match value {
            "success" => Ok(TestResult::Success),
            "aborted" => Ok(TestResult::Aborted),
            "fatal" => Ok(TestResult::FatalError),
            "fatal_error" => Ok(TestResult::FatalError),
            "inprogress" => Ok(TestResult::InProgress),
            "error_unknown" => Ok(TestResult::ErrorUnknown),
            "error_electrical" => Ok(TestResult::ErrorElectrical),
            "error_servo" => Ok(TestResult::ErrorServo),
            "error_read" => Ok(TestResult::ErrorRead),
            "error_handling" => Ok(TestResult::ErrorHandling),
            "ctrl_reset" => Ok(TestResult::CtrlReset),
            "ns_removed" => Ok(TestResult::NsRemoved),
            "aborted_format" => Ok(TestResult::AbortedFormat),
            "unknown_seg_fail" => Ok(TestResult::UnknownSegmentFailed),
            "known_seg_fail" => Ok(TestResult::KnownSegmentFailed),
            "aborted_unknown" => Ok(TestResult::AbortedUnknown),
            "aborted_sanitize" => Ok(TestResult::AbortedSanitize),
            _ => Err(()),
        }
    }
}
