
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::Win32::Foundation::HWND;




// we'll define our own new-type of Hwnd mostly coz HWND doesnt implement Debug, Hash etc
# [ derive (Debug, Default, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash) ]
pub struct Hwnd (pub isize);

impl Hwnd {
    pub fn is_valid (&self) -> bool { self.0 != 0 }
}

impl From <HWND> for Hwnd {
    fn from (hwnd:HWND) -> Self { Hwnd(hwnd.0 as _) }
}
impl From <Hwnd> for HWND {
    fn from (hwnd:Hwnd) -> Self { HWND(hwnd.0 as _) }
}
impl From <Hwnd> for isize {
    fn from (hwnd:Hwnd) -> Self { hwnd.0 }
}
impl From <isize> for Hwnd {
    fn from (hwnd: isize) -> Self { Hwnd(hwnd) }
}




// and the atomic version of Hwnd for storage
# [ derive (Debug, Default) ]
pub struct HwndAtomic (AtomicIsize);

impl HwndAtomic {
    pub fn load (&self) -> Hwnd {
        self.0.load (Ordering::Acquire) .into()
    }
    pub fn store (&self, hwnd: impl Into<Hwnd>) {
        self.0 .store (hwnd.into().0, Ordering::Release)
    }
    pub fn clear (&self) {
        self.store (Hwnd(0))
    }
    pub fn contains (&self, hwnd: impl Into<Hwnd>) -> bool {
        self.load() == hwnd.into()
    }
    pub fn is_valid (&self) -> bool {
        self.load() != Hwnd(0)
    }
}
impl From <HwndAtomic> for Hwnd {
    fn from (h_at: HwndAtomic) -> Hwnd { h_at.load() }
}
impl From <HwndAtomic> for HWND {
    fn from (h_at: HwndAtomic) -> HWND { h_at.load().into() }
}




/// representation for all our atomic flags for states mod-states, modifier-keys, mouse-btn-state etc <br>
/// (Note that this uses Acquire/Release memory ordering semantics, and shouldnt be used as lock/mutex etc)
# [ derive (Debug, Default) ]
pub struct Flag (AtomicBool);
// ^^ simple sugar that helps reduce clutter in code

impl Flag {
    /* Note regarding Atomic Memory Ordering usage here ..
       - The Flag struct is intended for use as simple flags, not as synchronization primitives (i.e locks)
       - On x86, there is strong memory model and Acq/Rel is free .. so no benefit to using Relaxed
       - SeqCst however requires a memory fence that could be potentially be costly (flush writes before atomic op etc)
       - For the very rare cases that would require total global ordering with SeqCst, we should just use lib facilities instead!!
    */
    pub fn new (state:bool) -> Flag { Flag (AtomicBool::new(state)) }

    /// toggling returns prior state .. better to use this than to check and set
    pub fn toggle (&self) -> bool { self.0 .fetch_xor (true, Ordering::AcqRel) }

    /// swap stores new state and returns prior state .. better to use this than to update and check/load separately
    pub fn swap   (&self, state:bool) -> bool { self.0 .swap (state, Ordering::AcqRel) }

    pub fn set   (&self) { self.0 .store (true,  Ordering::Release) }
    pub fn clear (&self) { self.0 .store (false, Ordering::Release) }

    pub fn store  (&self, state:bool) { self.0.store (state, Ordering::Release) }

    pub fn is_set   (&self) -> bool {  self.0 .load (Ordering::Acquire) }
    pub fn is_clear (&self) -> bool { !self.0 .load (Ordering::Acquire) }
}
impl From<Flag> for bool {
    fn from (flag: Flag) -> bool { flag.is_set() }
}

