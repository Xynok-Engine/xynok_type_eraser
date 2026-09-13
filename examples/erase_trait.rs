use std::sync::atomic::{AtomicBool, Ordering};

use xynok_type_eraser::erase_trait;

trait Test
{
    fn add(&self) -> u32;
    fn sub(&self) -> u32;
}

static DROPPED: AtomicBool = AtomicBool::new(false);

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

impl Drop for MyData
{
    fn drop(&mut self)
    {
        DROPPED.store(true, Ordering::SeqCst);
    }
}

erase_trait!(VTable, ErasedObject, Test {
    fn add(&self) -> u32;
    fn sub(&self) -> u32;
});

fn main()
{
    {
        let erased = ErasedObject::new(MyData { a: 1, b: 2 });
        println!("Add: 1 + 2 = {}", erased.add());
        println!("Sub: 1 - 2 = {}", erased.sub());
    }
    println!("Dropped: {}", DROPPED.load(Ordering::SeqCst));
}
