#![feature(allocator_api,iter_collect_into,slice_pattern)]

use core::slice::SlicePattern;
use std::io::{stdout, Write};

use ctru::prelude::*;
use ctru::services::gfx::{Flush, RawFrameBuffer, Screen, TopScreen3D};
use citro3d::macros::include_shader;
use citro3d::math::{AspectRatio, ClipPlanes, Projection, StereoDisplacement};
use citro3d::render::{ClearFlags, DepthFormat};
use citro3d::texenv;
use citro3d::{attrib, buffer, render, shader};
use glam::f32::{Vec3, Vec4};
use citro3d::math::Matrix4;
use gltf::Semantic;

#[repr(C)]
#[derive(Copy, Clone)]
struct Vertex {
    pos: Vec3,
    color: Vec3,
}

static VERTICES: &[Vertex] = &[
    Vertex {
        pos: Vec3::new(0.0, 0.5, 0.0),
        color: Vec3::new(1.0, 0.0, 0.0),
    },
    Vertex {
        pos: Vec3::new(-0.5, -0.5, 0.0),
        color: Vec3::new(0.0, 1.0, 0.0),
    },
    Vertex {
        pos: Vec3::new(0.5, -0.5, 0.0),
        color: Vec3::new(0.0, 0.0, 1.0),
    },
];

static DOC: &[u8] = include_bytes!("../assets/creature.glb");

static SHADER_BYTES: &[u8] = include_shader!("../assets/vshader.pica");
const CLEAR_COLOR: u32 = 0x68_B0_D8_FF;

static LOWER_SCREEN_RES: (u16, u16) = (320u16, 240u16);

fn main() {
    let apt = Apt::new().unwrap();
    let mut hid = Hid::new().unwrap();
    let gfx = Gfx::with_formats_shared(ctru::services::gspgpu::FramebufferFormat::Rgba8, ctru::services::gspgpu::FramebufferFormat::Rgb565).unwrap();
    let _console = Console::new(gfx.bottom_screen.borrow_mut());
    println!("Initializing...");
    let mut instance = match citro3d::Instance::new() {
        Ok(this) => {
            println!("Initialized citro3d...");
            this
        },
        Err(e) => {
            println!("ERROR: Failed to initilize citro3d!");
            println!("\t{:?}",e);
            panic!();
        }
    };
    

    let top_screen = TopScreen3D::from(&gfx.top_screen);

    let (mut top_left, mut top_right) = top_screen.split_mut(); 
    let RawFrameBuffer { width, height, .. } = top_left.raw_framebuffer();
    let mut top_left_target = instance
        .render_target(width, height, top_left, Some(DepthFormat::Depth16))
        .expect("failed to create render target");

    let RawFrameBuffer { width, height, .. } = top_right.raw_framebuffer();
    let mut top_right_target = instance
        .render_target(width, height, top_right, Some(DepthFormat::Depth16))
        .expect("failed to create render target");

    let shader = shader::Library::from_bytes(SHADER_BYTES).unwrap();
    let vertex_shader = shader.get(0).unwrap();

    let program = shader::Program::new(vertex_shader).unwrap();
    instance.bind_program(&program);

    let mut vbo_data = Vec::with_capacity_in(VERTICES.len()*2, ctru::linear::LinearAllocator);
    let cur = VERTICES.iter();
    vbo_data.extend(cur.clone().chain(cur.rev()));

    let mut buf_info = buffer::Info::new();
    let (attr_info, vbo_data) = prepare_vbos(&mut buf_info, &vbo_data);

    // Configure the first fragment shading substage to just pass through the vertex color
    // See https://www.opengl.org/sdk/docs/man2/xhtml/glTexEnv.xml for more insight
    let stage0 = texenv::Stage::new(0).unwrap();
    instance
        .texenv(stage0)
        .src(texenv::Mode::BOTH, texenv::Source::PrimaryColor, None, None)
        .func(texenv::Mode::BOTH, texenv::CombineFunc::Replace);

    let projection_uniform_idx = program.get_uniform("projection").unwrap();


    println!("Hello, World!");
    println!("\x1b[29;16HPress Start to exit");

    let mut model = Matrix4::identity();

    let mut view = Matrix4::identity();

    view.translate(0.0, 0.0, -4.0);
    

    while apt.main_loop() {
        gfx.wait_for_vblank();

        hid.scan_input();
        if hid.keys_down().contains(KeyPad::START) {
            break;
        }

        // Fun deadzone handling
        let (cx, cy) = 
        { 
            let cpos = hid.circlepad_position();

            println!("\x1b[4;0H\x1b[2Krawx: {}, rawy: {}", cpos.0, cpos.1);

            (match (cpos.0 as f32) / (i8::MAX as f32) {
                -0.1 ..= 0.1 => 0.0,
                a => a 
            },
            match (cpos.1 as f32) / (i8::MAX as f32) {
                -0.1 ..= 0.1 => 0.0,
                a => a 
            })
        };

        view.translate(cx, 0.0 , cy);

        let (tx, ty) =         { 
            let tpos = hid.touch_position();

            ((((tpos.0 as f32) / (LOWER_SCREEN_RES.0 as f32)) * 2.0) - 1.0,
            (((tpos.1 as f32) / (LOWER_SCREEN_RES.1 as f32)) * 2.0) - 1.0)
        };
        
        if hid.keys_down().contains(KeyPad::TOUCH) | hid.keys_held().contains(KeyPad::TOUCH) {
            model.rotate_z(tx);
            model.rotate_x(ty);
        }    
        print!("\x1b[5;0H\x1b[2Kcx: {}, cy: {}\r", cx, cy);
        
        instance.render_frame_with(|instance| {
            let mut render_to = |target: &mut render::Target, projection| {
                target.clear(ClearFlags::ALL, CLEAR_COLOR, 0);

                instance
                    .select_render_target(target)
                    .expect("failed to set render target");

                instance.bind_vertex_uniform(projection_uniform_idx, projection);

                instance.set_attr_info(&attr_info);

                instance.draw_arrays(buffer::Primitive::Triangles, vbo_data);
            };

            let Projections {
                left_eye,
                right_eye,
                _center,
            } = calculate_projections();

            let left_eye_mvp = left_eye * view * model; 
            let right_eye_mvp = right_eye * view * model;

            render_to(&mut top_left_target, &left_eye_mvp);
            render_to(&mut top_right_target, &right_eye_mvp);
        });

        stdout().flush().unwrap();
    }
}

fn prepare_vbos<'a>(
    buf_info: &'a mut buffer::Info,
    vbo_data: &'a [Vertex],
) -> (attrib::Info, buffer::Slice<'a>) {
    // Configure attributes for use with the vertex shader
    let mut attr_info = attrib::Info::new();

    let reg0 = attrib::Register::new(0).unwrap();
    let reg1 = attrib::Register::new(1).unwrap();

    attr_info
        .add_loader(reg0, attrib::Format::Float, 3)
        .unwrap();

    attr_info
        .add_loader(reg1, attrib::Format::Float, 3)
        .unwrap();

    let buf_idx = buf_info.add( vbo_data, &attr_info).unwrap();

    (attr_info, buf_idx)
}

struct Projections {
    left_eye: Matrix4,
    right_eye: Matrix4,
    _center: Matrix4,
}

fn calculate_projections() -> Projections {
    // TODO: it would be cool to allow playing around with these parameters on
    // the fly with D-pad, etc.
    let slider_val = ctru::os::current_3d_slider_state();
    let interocular_distance = slider_val / 2.0;

    let vertical_fov = 40.0_f32.to_radians();
    let screen_depth = 2.0;

    let clip_planes = ClipPlanes {
        near: 0.01,
        far: 100.0,
    };

    let (left, right) = StereoDisplacement::new(interocular_distance, screen_depth);

    let (left_eye, right_eye) =
        Projection::perspective(vertical_fov, AspectRatio::TopScreen, clip_planes)
            .stereo_matrices(left, right);

    let _center =
        Projection::perspective(vertical_fov, AspectRatio::BottomScreen, clip_planes).into();

    Projections {
        left_eye,
        right_eye,
        _center,
    }
}