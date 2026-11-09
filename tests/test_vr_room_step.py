"""VR room movement support regression, with a finite floor and no game resources.

Extract the production adapter. The engine seam models Entity.move's documented
onGround reset for a zero-Y move; support queries use a finite geometric floor.
This isolates the VR correction without changing the shared movement kernel.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_world_input_capture import function

ROOT = Path(__file__).resolve().parents[1]

class RoomStepTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which('rustc'), 'requires Rust')
    def test_head_motion_preserves_supported_jumps_but_not_edges_or_airborne_state(self):
        actual = function((ROOT / 'crates/horizon-client/src/motion.rs').read_text(), 'room_step')
        fixture = r'''
type Result<T> = std::result::Result<T, ()>;
#[derive(Clone, Copy)] struct Vec3 { x: f64, y: f64, z: f64 }
impl Vec3 { fn new(x:f64,y:f64,z:f64)->Self {Self{x,y,z}} }
struct Aabb { min: Vec3, max: Vec3 }
impl Aabb { fn new(min:Vec3,max:Vec3)->Self {Self{min,max}} fn min_position(&self)->Vec3 {self.min} fn max_position(&self)->Vec3 {self.max} fn move_by(self,v:Vec3)->Self {Self {
 min:Vec3::new(self.min.x+v.x,self.min.y+v.y,self.min.z+v.z),
 max:Vec3::new(self.max.x+v.x,self.max.y+v.y,self.max.z+v.z)}} }
struct Ground { on_ground: bool }
struct Collision { below: bool, horizontal: bool }
struct Player { ground: Ground, collision: Collision, position: Vec3, velocity: Vec3, spectator: bool }
impl Player { fn bounds(&self)->Aabb {let p=self.position; Aabb {
 min:Vec3::new(p.x-0.3,p.y,p.z-0.3), max:Vec3::new(p.x+0.3,p.y+1.8,p.z+0.3)}} }
struct World { player: Player }
enum MoverType { Player }
mod entity_move { use super::*;
 fn _documented_zero_y_move() {}
 pub(super) fn move_entity(w:&mut World,_:MoverType,v:Vec3)->Result<()> {
  w.player.position=Vec3::new(w.player.position.x+v.x,w.player.position.y+v.y,w.player.position.z+v.z);
  w.player.ground.on_ground=false; w.player.collision.below=false; Ok(())
 }
}
impl World {
 METHOD
 fn no_player_collision(&mut self,b:Aabb)->Result<bool> {
  // A two-by-two platform, with genuine free space past all four edges.
  Ok(!(b.min.y < 0.0 && b.max.y > -0.2 && b.min.x < 1.0 && b.max.x > -1.0 && b.min.z < 1.0 && b.max.z > -1.0))
 }
 fn set_on_ground_with_movement(&mut self,on:bool,_:bool,_:Vec3)->Result<()> { self.player.ground.on_ground=on; Ok(()) }
}
fn standing()->World { World { player: Player {ground:Ground{on_ground:true},collision:Collision{below:true,horizontal:false},position:Vec3::new(0.0,0.0,0.0),velocity:Vec3::new(0.0,0.0,0.0),spectator:false}} }
fn main() {
 let mut w=standing();
 // Repeated small room displacements caused by turning / leaning in the headset.
 for i in 0..120 { w.room_step(Vec3::new(if i%2==0 {0.004} else {-0.004},0.0,0.002)).unwrap(); assert!(w.player.ground.on_ground); }
 assert!(w.player.collision.below);
 // Both jump inputs can still find solid ground after the room move.
 w.player.velocity.y=0.42;
 w.room_step(Vec3::new(0.004,0.0,0.0)).unwrap();
 assert!(!w.player.ground.on_ground, "never re-ground a rising jump");
 let mut edge=standing(); edge.room_step(Vec3::new(2.0,0.0,0.0)).unwrap();
 assert!(!edge.player.ground.on_ground, "walking off an edge must remain airborne");
 let mut air=standing(); air.player.position.y=0.5; air.player.ground.on_ground=false;
 air.room_step(Vec3::new(0.004,0.0,0.0)).unwrap(); assert!(!air.player.ground.on_ground);
 let mut lifted=standing(); lifted.room_step(Vec3::new(0.0,0.2,0.0)).unwrap(); assert!(!lifted.player.ground.on_ground);
 let mut spectator=standing(); spectator.player.spectator=true;
 spectator.room_step(Vec3::new(0.004,0.0,0.0)).unwrap(); assert!(!spectator.player.ground.on_ground);
}
'''.replace('METHOD', actual)
        with tempfile.TemporaryDirectory(prefix='horizon-vr-room-') as directory:
            source, binary = Path(directory)/'fixture.rs', Path(directory)/'fixture'
            source.write_text(fixture)
            build=subprocess.run(['rustc','--edition=2024',str(source),'-o',str(binary)],capture_output=True,text=True)
            self.assertEqual(build.returncode,0,build.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True)
            self.assertEqual(run.returncode,0,run.stderr)
