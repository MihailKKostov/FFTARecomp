use crate::engine::{self, Engine, ENGINE, KEYS, STATUS};
use jni_sys::*;
use std::{path::PathBuf, sync::atomic::Ordering};

/// UI piece slots (9 floats each) followed by the padded-frame flag; GameSurface mirrors it.
const UI_PARAMS:usize=crate::touch::UI_SLOTS*9+4;

fn guarded(action: impl FnOnce() -> Result<i32, String>) -> i32 {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => { engine::status(format!("ERROR: {error}")); -1 },
        Err(_) => { engine::status("ERROR: native engine panicked; reload the ROM"); -1 },
    }
}

unsafe fn string(env: *mut JNIEnv, value: jstring) -> Result<String, String> {
    if value.is_null() { return Err("Missing path".into()); }
    let api = &**env;
    let len = api.GetStringLength.unwrap()(env,value) as usize;
    let data = api.GetStringChars.unwrap()(env,value,std::ptr::null_mut());
    if data.is_null() { return Err("Cannot read Java string".into()); }
    let text = String::from_utf16(std::slice::from_raw_parts(data,len)).map_err(|e|e.to_string());
    api.ReleaseStringChars.unwrap()(env,value,data);
    text
}

unsafe fn java_string(env: *mut JNIEnv, text: &str) -> jstring {
    let utf16: Vec<_> = text.encode_utf16().collect();
    (&**env).NewString.unwrap()(env,utf16.as_ptr(),utf16.len() as i32)
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaLoadRom(
    env: *mut JNIEnv, _: jobject, bytes: jbyteArray, save_path: jstring,
) -> jint {
    guarded(|| {
        if bytes.is_null() { return Err("No ROM bytes".into()); }
        let api = &**env;
        let len = api.GetArrayLength.unwrap()(env,bytes);
        if !(192..=32*1024*1024).contains(&len) { return Err("Invalid ROM size".into()); }
        let mut rom = vec![0u8;len as usize];
        api.GetByteArrayRegion.unwrap()(env,bytes,0,len,rom.as_mut_ptr() as *mut _);
        if api.ExceptionCheck.unwrap()(env) != 0 { return Err("Cannot read ROM array".into()); }
        if !engine::verify_rom(&rom) { return Err("ROM SHA1/SHA256 mismatch: exact FFTA USA or Europe is required".into()); }
        let save = PathBuf::from(string(env,save_path)?);
        let mut loaded = Engine::new(rom);
        if save.is_file() {
            let bytes = std::fs::read(&save).map_err(|e|format!("Read save: {e}"))?;
            loaded.machine.bus.load_save_data(&bytes);
        }
        loaded.note_battery();
        loaded.save_path = Some(save);
        loaded.cache_dir = loaded.save_path.as_ref().and_then(|path|path.parent()).map(|path|path.join("compiled"));
        *ENGINE.lock().map_err(|_|"Engine mutex poisoned")? = Some(loaded);
        KEYS.store(0x3ff,Ordering::Relaxed);
        engine::status("ROM verified and loaded; ready to compile");
        Ok(1)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaPrepare(_: *mut JNIEnv, _: jobject) -> jint {
    guarded(|| {
        let mut lock = ENGINE.lock().map_err(|_|"Engine mutex poisoned")?;
        lock.as_mut().ok_or("Load a ROM first")?.prepare()?;
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaFrame(
    env: *mut JNIEnv, _: jobject, keys: jint, extra_x: jint, extra_y: jint,
    frames: jint, sound: jboolean, audio_mode:jint, pixels: jobject, audio: jshortArray,
    touch:jboolean, base_width:jfloat, ui_size:jfloat, ui_length:jfloat,
    screen_width:jint,screen_height:jint,fill:jboolean,ui:jobject,ui_params:jfloatArray,ui_background:jobject,
    ui_top:jfloat,ui_bottom:jfloat,
) -> jint {
    guarded(|| {
        if pixels.is_null() || audio.is_null() { return Err("Missing frame buffers".into()); }
        if !(0..=gba_core::mem::MAX_EXTRA as i32).contains(&extra_x)
            || !(0..=gba_core::mem::MAX_EXTRA_Y as i32).contains(&extra_y)
            || !(1..=8).contains(&frames) { return Err("Invalid render configuration".into()); }
        KEYS.store(keys as u32 & 0x3ff,Ordering::Relaxed);
        let api = &**env;
        let size = (240+2*extra_x)*(160+2*extra_y);
        if api.GetDirectBufferCapacity.unwrap()(env,pixels)<size as i64*4 || api.GetArrayLength.unwrap()(env,audio)<4096 {
            return Err("Frame buffer size mismatch".into());
        }
        let target = api.GetDirectBufferAddress.unwrap()(env,pixels) as *mut i32;
        if target.is_null() { return Err("Expected a direct pixel buffer".into()); }
        let mut lock = ENGINE.lock().map_err(|_|"Engine mutex poisoned")?;
        let engine = lock.as_mut().ok_or("ROM not loaded")?;
        engine.configure_touch(touch!=0,base_width,ui_size,ui_length);
        if screen_width<=0 || screen_height<=0 {return Err("Invalid screen size".into());}
        engine.ui_dock=(ui_top.is_finite()&&ui_bottom.is_finite()&&ui_bottom>ui_top).then_some((ui_top,ui_bottom));
        // Taps arrive between frames, so they map through the frame the player saw last.
        engine.set_view((screen_width as u32,screen_height as u32),extra_x as usize,extra_y as usize,fill!=0);
        engine.frame_audio(frames as usize, extra_x as usize, extra_y as usize,
            if sound==0{0}else if audio_mode==2{2}else{1})?;
        std::ptr::copy_nonoverlapping(engine.pixels.as_ptr(),target,size as usize);
        if ui.is_null() || ui_background.is_null() || ui_params.is_null()
            || api.GetDirectBufferCapacity.unwrap()(env,ui)<(480*160*4*2) as i64
            || api.GetDirectBufferCapacity.unwrap()(env,ui_background)<size as i64*4
            || api.GetArrayLength.unwrap()(env,ui_params)<UI_PARAMS as i32 {return Err("Invalid UI buffers".into());}
        let ui_ptr=api.GetDirectBufferAddress.unwrap()(env,ui) as *mut u8;
        let bg_ptr=api.GetDirectBufferAddress.unwrap()(env,ui_background) as *mut i32;
        if ui_ptr.is_null()||bg_ptr.is_null(){return Err("Expected direct UI buffers".into());}
        let mut params=[0f32;UI_PARAMS];
        let ui_start=engine.machine.bus.ffta_dialogue.profile.enabled.then(std::time::Instant::now);
        engine.export_ui(std::slice::from_raw_parts_mut(ui_ptr,480*160*4*2),&mut params,
            std::slice::from_raw_parts_mut(bg_ptr,size as usize));
        if let Some(start)=ui_start {engine.ui_export_ns=start.elapsed().as_nanos() as u64;}
        api.SetFloatArrayRegion.unwrap()(env,ui_params,0,UI_PARAMS as i32,params.as_ptr());
        let count = engine.audio.len().min(4096) as i32;
        if count > 0 { api.SetShortArrayRegion.unwrap()(env,audio,0,count,engine.audio.as_ptr()); }
        if api.ExceptionCheck.unwrap()(env) != 0 { return Err("Cannot copy frame data".into()); }
        Ok(count)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaUiProfile(_: *mut JNIEnv,_:jobject,enabled:jboolean,reference:jboolean){
    if let Ok(mut lock)=ENGINE.lock(){if let Some(engine)=lock.as_mut(){
        engine.machine.bus.ffta_dialogue.profile.enabled=enabled!=0;
        engine.machine.bus.ffta_dialogue.reference_capture=reference!=0;
    }}
}
#[no_mangle]
pub extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaPreferPerformanceCpu(_: *mut JNIEnv,_:jobject)->jboolean {
    crate::scheduling::prefer_performance_cpus() as jboolean
}
#[no_mangle]
pub extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaReferenceDialogue(_: *mut JNIEnv,_:jobject,reference:jboolean){
    if let Ok(mut lock)=ENGINE.lock(){if let Some(engine)=lock.as_mut(){
        engine.machine.bus.ffta_dialogue.reference_dialogue=reference!=0;
    }}
}
#[no_mangle]
pub extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaReferenceFrame(_: *mut JNIEnv,_:jobject,reference:jboolean){
    if let Ok(mut lock)=ENGINE.lock(){if let Some(engine)=lock.as_mut(){
        engine.machine.bus.ffta_dialogue.reference_frame=reference!=0;
    }}
}
#[no_mangle]
pub unsafe extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaUiTimings(env:*mut JNIEnv,_:jobject,out:jlongArray){
    if out.is_null(){return;}
    let api=&**env;
    if api.GetArrayLength.unwrap()(env,out)<7{return;}
    if let Ok(lock)=ENGINE.lock(){if let Some(engine)=lock.as_ref(){
        let p=&engine.machine.bus.ffta_dialogue.profile;
        let values=[p.capture_ns as i64,p.setup_ns as i64,engine.ui_export_ns as i64,p.rows as i64,
            engine.machine.bus.ffta_dialogue.layer().is_some() as i64,p.raster_ns as i64,p.zoom_ns as i64];
        api.SetLongArrayRegion.unwrap()(env,out,0,7,values.as_ptr());
    }}
}

/// World map Menu button rectangle (x, y, width, height in surface pixels); false when hidden.
#[no_mangle]
pub unsafe extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaMenuButton(env:*mut JNIEnv,_:jobject,out:jfloatArray)->jboolean{
    if out.is_null(){return 0;}
    let api=&**env;
    if api.GetArrayLength.unwrap()(env,out)<4{return 0;}
    // Only when the game picture could not carry the button itself.
    let rect=ENGINE.lock().ok().and_then(|lock|lock.as_ref().and_then(|engine|engine.menu_button.filter(|_|!engine.menu_button_in_game)));
    match rect {
        Some(rect)=>{api.SetFloatArrayRegion.unwrap()(env,out,0,4,rect.as_ptr());1}
        None=>0,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaTap(_: *mut JNIEnv,_:jobject,x:jfloat,y:jfloat,back:jboolean){
    guarded(||{
        if let Some(engine)=ENGINE.lock().map_err(|_|"Engine mutex poisoned")?.as_mut(){engine.tap(x,y,back!=0);}
        Ok(1)
    });
}

#[no_mangle]
pub extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaSave(_: *mut JNIEnv, _: jobject) -> jint {
    guarded(|| {
        if let Some(engine) = ENGINE.lock().map_err(|_|"Engine mutex poisoned")?.as_mut() { engine.save()?; }
        Ok(1)
    })
}

/// Pause, exit, background: resume state and battery save right away.
#[no_mangle]
pub extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaCheckpoint(_: *mut JNIEnv, _: jobject) -> jint {
    guarded(|| {
        if let Some(engine) = ENGINE.lock().map_err(|_|"Engine mutex poisoned")?.as_mut() { engine.checkpoint()?; }
        Ok(1)
    })
}

/// After loading the ROM: continue where the game was left (1), or start from the battery save (0).
#[no_mangle]
pub extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaResume(_: *mut JNIEnv, _: jobject) -> jint {
    let resumed = ENGINE.lock().ok().and_then(|mut lock| lock.as_mut().map(|engine| engine.resume()));
    match resumed {
        Some(Ok(true)) => { engine::status("Resumed where the game was left"); 1 }
        Some(Err(error)) => { engine::status(format!("Resume state not used: {error}")); 0 }
        _ => 0,
    }
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaStatus(env: *mut JNIEnv, _: jobject) -> jstring {
    java_string(env,&STATUS.lock().unwrap_or_else(|e|e.into_inner()))
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaDiagnostics(env: *mut JNIEnv, _: jobject) -> jstring {
    let text = match ENGINE.try_lock() {
        Ok(engine) => engine.as_ref().map_or_else(||"No ROM loaded".into(), Engine::diagnostics),
        Err(_) => "Engine busy compiling/running".into(),
    };
    java_string(env,&text)
}

#[no_mangle]
pub extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaCamera(_: *mut JNIEnv,_:jobject)->jlong{
    let lock=ENGINE.lock().unwrap_or_else(|e|e.into_inner());
    let Some(engine)=lock.as_ref()else{return -1;};
    let io=&engine.machine.bus.io;
    for bg in 0..4{
        let cnt=u16::from_le_bytes([io[8+bg*2],io[9+bg*2]]);
        if cnt==0x4c03 || cnt==0x4e02{
            let h=u16::from_le_bytes([io[16+bg*4],io[17+bg*4]])&511;
            let v=u16::from_le_bytes([io[18+bg*4],io[19+bg*4]])&511;
            return h as i64 | ((v as i64)<<9);
        }
    }
    -1
}

#[no_mangle]
pub extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaParallelZoom(_: *mut JNIEnv,_:jobject,enabled:jboolean){
    gba_core::ppu::set_parallel_zoom(enabled!=0);
}

#[no_mangle]
pub extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaNormalAudioReader(_: *mut JNIEnv,_:jobject,enabled:jboolean){crate::normal_audio::set_external_reader(enabled!=0);}
#[no_mangle]
pub unsafe extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaPullNormalAudio(env:*mut JNIEnv,_:jobject,audio:jshortArray)->jint{
    guarded(||{
        if audio.is_null(){return Err("Missing audio buffer".into());}
        let mut samples=[0i16;2048];
        let capacity=(&**env).GetArrayLength.unwrap()(env,audio).max(0) as usize;
        let count=crate::normal_audio::pull(&mut samples[..capacity.min(2048)]).map_err(|e|e.to_string())?;
        if count>0{(&**env).SetShortArrayRegion.unwrap()(env,audio,0,count as i32,samples.as_ptr());}
        Ok(count as i32)
    })
}
#[no_mangle]
pub extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaCharacterMetrics(_: *mut JNIEnv,_:jobject,enabled:jboolean){gba_core::ffta_sprites::set_character_metrics(enabled!=0);}
#[no_mangle]
pub unsafe extern "system" fn Java_com_fftarecomp_port_NativeBridge_gbaCharacters(env:*mut JNIEnv,_:jobject)->jstring{
    let lock=ENGINE.lock().unwrap_or_else(|e|e.into_inner());
    let text=lock.as_ref().map(|e|gba_core::ffta_sprites::tutorial_metric(&e.machine.bus)).unwrap_or_default();
    java_string(env,&text)
}
