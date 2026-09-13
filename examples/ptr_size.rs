use std::mem::size_of;
trait SomeTrait {}

fn main()
{
    println!("======== The size of different pointers in Rust: ========");
    println!("{:<20}{}", "&dyn Trait:", size_of::<&dyn SomeTrait>());
    println!("{:<20}{}", "&[&dyn Trait]:", size_of::<&[&dyn SomeTrait]>());
    println!("{:<20}{}", "Box<Trait>:", size_of::<Box<dyn SomeTrait>>());
    println!("{:<20}{}", "Box<Box<Trait>>:", size_of::<Box<Box<dyn SomeTrait>>>());
    println!("{:<20}{}", "&u32:", size_of::<&u32>());
    println!("{:<20}{}", "&[u32]:", size_of::<&[u32]>());
    println!("{:<20}{}", "usize:", size_of::<usize>());
    println!("{:<20}{}", "&usize:", size_of::<&usize>());
    println!("{:<20}{}", "&[usize]:", size_of::<&[usize]>());
    println!("{:<20}{}", "&i32:", size_of::<&i32>());
    println!("{:<20}{}", "&[i32]:", size_of::<&[i32]>());
    println!("{:<20}{}", "Box<i32>:", size_of::<Box<i32>>());
    println!("{:<20}{}", "&Box<i32>:", size_of::<&Box<i32>>());
    println!("{:<20}{}", "[&dyn Trait;4]:", size_of::<[&dyn SomeTrait; 4]>());
    println!("{:<20}{}", "[i32;4]:", size_of::<[i32; 4]>());
}
