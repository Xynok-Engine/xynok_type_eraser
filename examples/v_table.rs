#![allow(unused)]

trait Test
{
    fn add(&self) -> u32;
    fn sub(&self) -> u32;
}
struct VTable
{
    pub drop: unsafe fn(*mut ()),
    pub add:  unsafe fn(*const ()) -> u32,
    pub sub:  unsafe fn(*const ()) -> u32,
}
impl VTable
{
    pub const fn for_type<T: Test>() -> Self
    {
        unsafe fn drop_fn<T: Test>(ptr: *mut ())
        {
            unsafe {
                drop(Box::from_raw(ptr as *mut T));
            }
        }
        unsafe fn add_fn<T: Test>(ptr: *const ()) -> u32
        {
            unsafe { (*(ptr as *const T)).add() }
        }
        unsafe fn sub_fn<T: Test>(ptr: *const ()) -> u32
        {
            unsafe { (*(ptr as *const T)).sub() }
        }

        Self {
            drop: drop_fn::<T>,
            add:  add_fn::<T>,
            sub:  sub_fn::<T>,
        }
    }
}

struct ErasedObject
{
    data:   *mut (),
    vtable: &'static VTable,
}
impl Drop for ErasedObject
{
    fn drop(&mut self)
    {
        unsafe { (self.vtable.drop)(self.data) };
    }
}
impl ErasedObject
{
    pub fn new<T: Test>(value: T) -> Self
    {
        let boxed = Box::new(value);
        Self {
            data:   Box::into_raw(boxed) as *mut (),
            vtable: &const { VTable::for_type::<T>() },
        }
    }

    pub fn add(&self) -> u32
    {
        unsafe { (self.vtable.add)(self.data) }
    }

    pub fn sub(&self) -> u32
    {
        unsafe { (self.vtable.sub)(self.data) }
    }
}
struct MyData
{
    a: u32,
    b: u32,
}
impl Test for MyData
{
    fn add(&self) -> u32
    {
        self.a.wrapping_add(self.b)
    }

    fn sub(&self) -> u32
    {
        self.a.wrapping_sub(self.b)
    }
}
fn main()
{
    let erased = ErasedObject::new(MyData { a: 1, b: 2 });
    println!("Add: {}", erased.add());
    println!("Sub: {}", erased.sub());
    // erased bị drop ở cuối scope, tự động gọi vtable.drop, giải phóng MyData đúng cách
}
