use super::*;

#[derive(Copy, Clone, ConstDefault, ConstParamTy, Eq, PartialEq)]
pub struct IRanged<const NB: u32, const NE: u32> {
    i: u32,
}

pub struct IRangedIter<const NB: u32, const NE: u32> {
    i: IRanged<NB, NE>,
    e: IRanged<NB, NE>,
    b: bool,
}

impl<const NB: u32, const NE: u32> IRangedIter<NB, NE> {
    fn init(i: IRanged<NB, NE>, e: IRanged<NB, NE>) -> Self {
        Self { i, e, b: false }
    }
}

impl<const NB: u32, const NE: u32> Iterator for IRangedIter<NB, NE>
where
    [(); (NE > NB) as usize - 1]:,
{
    type Item = IRanged<NB, NE>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.b {
            return None;
        }
        let r = self.i;
        if self.i == self.e {
            self.b = true;
        } else {
            self.i.next_wrapped_n(&self.e);
        }
        Some(r)
    }
}

impl<const NB: u32, const NE: u32> IRanged<NB, NE>
where
    [(); (NE > NB) as usize - 1]:,
{
    pub fn atomic(&mut self) -> &mut AtomicU32 {
        AtomicU32::from_mut(&mut self.i)
    }

    pub fn get_atomic(&mut self) -> Self {
        Self {
            i: self.atomic().load(Ordering::Acquire),
        }
    }

    pub fn set_atomic(&mut self, i: u32) {
        assert!(i >= NB && i <= NE);
        self.atomic().store(i, Ordering::Release)
    }

    pub fn all() -> IRangedIter<NB, NE> {
        Self::up_to(&Self::end())
    }

    pub fn up_to(n: &IRanged<NB, NE>) -> IRangedIter<NB, NE> {
        IRangedIter::<NB, NE>::init(Self::beg(), *n)
    }

    pub const fn init(i: u32) -> Self {
        assert!(i >= NB && i <= NE);
        unsafe { Self::init_unchecked(i) }
    }

    pub const unsafe fn init_unchecked(i: u32) -> Self {
        Self { i }
    }

    pub const fn get(&self) -> u32 {
        self.i
    }

    pub fn set(&mut self, i: u32) {
        assert!(i >= NB && i <= NE);
        self.i = i;
    }

    pub fn next_wrapped(&mut self) {
        self.next_wrapped_n(&Self::end())
    }

    pub fn prev_wrapped(&mut self) {
        self.prev_wrapped_n(&Self::end())
    }

    pub fn beg() -> Self {
        unsafe { Self::init_unchecked(NB) }
    }

    pub fn end() -> Self {
        unsafe { Self::init_unchecked(NE) }
    }

    pub fn prev_wrapped_n(&mut self, e: &IRanged<NB, NE>) {
        if *self == Self::beg() {
            *self = *e;
        } else {
            self.i -= 1
        }
    }

    pub fn next_wrapped_n(&mut self, e: &IRanged<NB, NE>) {
        if *self == *e {
            *self = Self::beg()
        } else {
            self.i += 1
        }
    }

    pub fn index_into_mut<'a, T, const N: usize>(&self, t: &'a mut [T; N]) -> &'a mut T
    where
        [(); ((NE as usize) < N) as usize - 1]:,
    {
        unsafe { t.get_unchecked_mut(self.i as usize) }
    }
}

pub type Ranged<const N: u32> = IRanged<0, { N - 1 }>;

#[derive(ConstDefault)]
pub struct RangedArray<T, const N: u32>
where
    [(); N as usize]:,
{
    a: [T; N as usize],
}

impl<T, const N: u32> RangedArray<T, N>
where
    [(); N as usize]:,
{
    pub fn arr(&mut self) -> &mut [T; N as usize] {
        &mut self.a
    }

    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.a.as_mut_ptr()
    }

    pub fn get<'a>(&'a self, i: &Ranged<N>) -> &'a T {
        unsafe { self.a.get_unchecked(i.i as usize) }
    }

    pub fn get_mut<'a>(&'a mut self, i: &Ranged<N>) -> &'a mut T {
        unsafe { self.a.get_unchecked_mut(i.i as usize) }
    }
}
