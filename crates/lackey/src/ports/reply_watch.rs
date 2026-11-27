//! Who is told while a question is answered or a claim checked: the editor shows it on the
//! region's line and in its status line, from the call's start to its end.

use crate::domain::region_record::RegionRecord;

pub trait ReplyWatch: Send + Sync {
    /// A reply to the region is being sought. Answers `false` when one already is (an earlier
    /// save started it), and then the region is left to that one.
    fn begins(&self, record: &RegionRecord) -> bool;
    /// The reply is in, or failed: the region's line stops saying it is being worked on.
    fn ends(&self, record: &RegionRecord);
}
