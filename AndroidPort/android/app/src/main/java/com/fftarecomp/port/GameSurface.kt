package com.fftarecomp.port

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.opengl.GLES20.*
import android.opengl.GLSurfaceView
import android.os.Process
import android.view.MotionEvent
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.FloatBuffer
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.locks.LockSupport
import javax.microedition.khronos.egl.EGLConfig
import javax.microedition.khronos.opengles.GL10

/** UI piece slots exported per frame (touch.rs UI_SLOTS), 9 floats each, then the padded-frame flag. */
private const val UI_SLOTS=8
private const val UI_PARAMS=UI_SLOTS*9+4

/** Simulation and audio run together off the UI thread; GL only uploads completed frames. */
class GameSurface(context: Context, val onError: (String) -> Unit) : GLSurfaceView(context) {
    companion object { val deliveredClicks=AtomicInteger(0) }
    val buttons = AtomicInteger(0)
    @Volatile var inputSource: AtomicInteger = buttons
    @Volatile var gamePaused = false
    @Volatile var appActive = true
    @Volatile var viewRequest = ViewRequest(AspectMode.DEVICE,0)
    @Volatile var speed = 1
    @Volatile var audioMode=FastAudio.NORMAL
    @Volatile var soundEnabled = true
    @Volatile var volume = 0.8f
    @Volatile var experimentalClicks=false
    @Volatile var touchOnly=false
    /** Receives the held GBA direction while a touch-only drag steers. */
    @Volatile var onDirection:(Int)->Unit={}
    /** Touch-only portrait world map: the Menu button's rectangle (px), null when hidden. */
    @Volatile var onMenuButton:(List<Float>?)->Unit={}
    private val menuBuffer=FloatArray(4)
    private var menuButton:List<Float>?=null
    /** Portrait battles: screen rows reserved above (px) and below (fraction of height, px inset) the UI frame. */
    @Volatile var dockTop=0f
    @Volatile var dockReserve=0f
    @Volatile var dockBottomInset=0f
    @Volatile var bubbleSize=1f
    @Volatile var bubbleLength=1f
    private val taps=java.util.concurrent.ConcurrentLinkedQueue<ScreenTap>()
    private var touchingGame=false
    private var dragging=false
    private var twoFinger=false
    private var downTime=0L
    private var downX=0f
    private var downY=0f
    /** The gesture scrolls the Party screen's unit grid (like a list) instead of steering. */
    private var scrolling=false
    private var lastY=0f
    // Compose sends touches on its buttons to those buttons, never this underlying View.
    // Only complete single-pointer gestures that began on the game can become clicks.
    // Touch-only mode adds two gestures to single taps: a one-finger drag holds the
    // D-pad direction of its offset, and a quick two-finger tap presses B (back).
    override fun onTouchEvent(event:MotionEvent):Boolean {
        if(!experimentalClicks || gamePaused || !appActive){endGesture();return false}
        val density=resources.displayMetrics.density
        val slop=(if(touchOnly)32 else 48)*density
        when(event.actionMasked){
            MotionEvent.ACTION_DOWN->{
                touchingGame=true;dragging=false;twoFinger=false
                downX=event.x;downY=event.y;downTime=event.eventTime;lastY=event.y
                scrolling=touchOnly && runCatching{NativeBridge.gbaDragStarts(event.x/width,event.y/height)}.getOrDefault(false)
            }
            MotionEvent.ACTION_POINTER_DOWN->
                if(touchOnly && touchingGame && !dragging && event.pointerCount==2)twoFinger=true
                else {touchingGame=false;onDirection(0)}
            MotionEvent.ACTION_MOVE->if(touchOnly && touchingGame && !twoFinger){
                val dx=event.x-downX;val dy=event.y-downY
                if(scrolling){
                    // Lists follow the finger as soon as it clearly moves.
                    if(!dragging && kotlin.math.abs(dy)>=slop/3)dragging=true
                    if(dragging){NativeBridge.gbaDrag((event.y-lastY)/height);lastY=event.y}
                }else{
                    if(!dragging && dx*dx+dy*dy>=slop*slop)dragging=true
                    if(dragging)onDirection(TouchSelection.direction(dx,dy,slop/2))
                }
            }
            MotionEvent.ACTION_CANCEL->endGesture()
            MotionEvent.ACTION_UP->{
                if(touchingGame && twoFinger){
                    if(event.eventTime-downTime<500 && taps.size<8)taps.offer(ScreenTap(downX/width,downY/height,true))
                }else if(touchingGame && !dragging && kotlin.math.abs(event.x-downX)<slop &&
                    kotlin.math.abs(event.y-downY)<slop){
                    val view=frames.displayedView
                    val point=TouchSelection.map(event.x,event.y,width,height,view)
                    if(point!=null && taps.size<8)taps.offer(ScreenTap(event.x/width,event.y/height,
                        event.buttonState and MotionEvent.BUTTON_SECONDARY!=0))
                }
                endGesture()
            }
        }
        return true
    }
    private fun endGesture(){
        if(dragging && !scrolling)onDirection(0)
        touchingGame=false;dragging=false;twoFinger=false;scrolling=false
    }
    @Volatile var performance = "Starting..."
    @Volatile private var running = false
    private var worker: Thread? = null
    private val frames = FrameRenderer(onError)

    init {
        setEGLContextClientVersion(2)
        preserveEGLContextOnPause = true
        setRenderer(frames)
        renderMode = RENDERMODE_WHEN_DIRTY
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        running = true
        worker = Thread(::runGame, "FFTA-game").apply { start() }
    }

    override fun onDetachedFromWindow() {
        running = false
        buttons.set(0)
        worker?.interrupt()
        worker?.join(2000)
        worker = null
        super.onDetachedFromWindow()
    }

    private fun runGame() {
        Process.setThreadPriority(Process.THREAD_PRIORITY_DISPLAY)
        val benchmark=GameBenchmark.create(context)
        NativeBridge.gbaNormalAudioReader(true)
        val normalOutput=NormalAudioOutput { message -> post { onError(message) } }
        // Apply only to simulation, after its independent audio thread exists.
        if(GameBenchmark.preferFastCpu(context))NativeBridge.gbaPreferPerformanceCpu()
        val sampleBuffer = ShortArray(4096)
        // Small bounded queue absorbs AudioTrack partial writes without accumulating latency.
        val queued = ShortArray(16384)
        var queuedCount = 0
        var back: ByteBuffer? = null
        var uiBack=UiBuffers(4)
        val track = try {
            val minimum = AudioTrack.getMinBufferSize(48000, AudioFormat.CHANNEL_OUT_STEREO,
                AudioFormat.ENCODING_PCM_16BIT).coerceAtLeast(4096)
            AudioTrack.Builder()
                .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_GAME)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
                .setAudioFormat(AudioFormat.Builder().setSampleRate(48000)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO)
                    .setEncoding(AudioFormat.ENCODING_PCM_16BIT).build())
                .setBufferSizeInBytes(minimum * 2).build()
        } catch (error: Exception) { post { onError("Audio initialization: ${error.message}") }; null }
        var audible = false
        var appliedVolume=Float.NaN
        var lastAudioKey=-1
        var deadline = System.nanoTime()
        var count = 0
        var sampleStart = deadline
        var simulationNanos = 0L
        var wasPaused = false
        try {
            while (running) {
                if (gamePaused || !appActive) {
                    taps.clear()
                    normalOutput.active=false
                    if (!wasPaused) {
                        // Paused, backgrounded or leaving: keep the exact moment to resume from.
                        NativeBridge.gbaCheckpoint()
                        buttons.set(0)
                        track?.pause(); track?.flush()
                        queuedCount = 0
                        audible = false
                        wasPaused = true
                    }
                    LockSupport.parkNanos(10_000_000)
                    deadline = System.nanoTime()
                    sampleStart = deadline; count = 0; simulationNanos = 0
                    continue
                }
                wasPaused = false
                val request=viewRequest
                val aspect=request.aspectFor(frames.screenWidth,frames.screenHeight)
                val config = benchmark?.view ?: ViewConfig.create(aspect,request.zoom,frames.screenWidth,frames.screenHeight)
                val bytes = config.width * config.height * 4
                if (back == null || back.capacity() != bytes) {
                    back = ByteBuffer.allocateDirect(bytes).order(ByteOrder.nativeOrder())
                }
                uiBack.resize(bytes)
                val audioCheck=benchmark!=null && benchmark.frame>=benchmark.frames && benchmark.audioFrames>0
                val multiplier = if(audioCheck)4 else if(benchmark!=null)1 else speed.coerceIn(1, 8)
                val mode=if(audioCheck)FastAudio.NORMAL else audioMode
                val sound = soundEnabled && (multiplier==1 || mode!=FastAudio.SILENT) && track != null
                val independentNormal=sound && multiplier>1 && mode==FastAudio.NORMAL
                normalOutput.volume=volume
                normalOutput.active=independentNormal
                val audioKey=if(!sound)0 else if(multiplier==1)1 else 10+mode.ordinal*10+multiplier
                if(audioKey!=lastAudioKey) {
                    track?.pause();track?.flush();queuedCount = 0;audible=false;lastAudioKey=audioKey
                }
                if(audioCheck)benchmark?.beginAudio()
                val start = System.nanoTime()
                if(!experimentalClicks)taps.clear()
                while(true){val tap=taps.poll()?:break;NativeBridge.gbaTap(tap.x,tap.y,tap.back);deliveredClicks.incrementAndGet()}
                val base=ViewConfig.create(aspect,0,frames.screenWidth,frames.screenHeight)
                val samples = NativeBridge.gbaFrame(benchmark?.keys() ?: (inputSource.get().inv() and 0x3ff),
                    config.extraX, config.extraY, multiplier, sound, if(mode==FastAudio.NORMAL)2 else 1, back, sampleBuffer,
                    experimentalClicks,base.width*160f/base.height,bubbleSize,bubbleLength,
                    frames.screenWidth,frames.screenHeight,config.fillScreen,uiBack.pixels,uiBack.params,uiBack.background,
                    dockTop,if(frames.screenHeight>frames.screenWidth)
                        frames.screenHeight-maxOf(dockBottomInset,dockReserve*frames.screenHeight) else 0f)
                val frameNanos=System.nanoTime()-start
                simulationNanos += frameNanos
                if (samples < 0) throw IllegalStateException(NativeBridge.gbaStatus())
                val menu=if(touchOnly&&NativeBridge.gbaMenuButton(menuBuffer))menuBuffer.toList() else null
                if(menu!=menuButton){menuButton=menu;onMenuButton(menu)}
                val recycled=frames.publish(back, config,uiBack)
                back=recycled.first
                uiBack=recycled.second?:UiBuffers(bytes)
                requestRender()
                val publicationEnd=if(benchmark!=null)System.nanoTime() else 0L
                if (sound && !independentNormal) {
                    if (!audible) { track?.play(); audible = true }
                    val desiredVolume=volume.coerceIn(0f,1f)
                    if(desiredVolume!=appliedVolume){track?.setVolume(desiredVolume);appliedVolume=desiredVolume}
                    if (queuedCount + samples > queued.size) queuedCount = 0
                    sampleBuffer.copyInto(queued, queuedCount, 0, samples)
                    queuedCount += samples
                    val written = track?.write(queued, 0, queuedCount, AudioTrack.WRITE_NON_BLOCKING) ?: 0
                    if (written > 0) {
                        queued.copyInto(queued, 0, written, queuedCount)
                        queuedCount -= written
                    }
                } else if (audible) {
                    track?.pause(); track?.flush(); queuedCount = 0; audible = false
                }
                val publishEnd=if(benchmark!=null)System.nanoTime() else 0L
                if(audioCheck)benchmark?.recordAudio(normalOutput)
                else benchmark?.record(frameNanos,track?.underrunCount ?: 0)
                if(benchmark!=null && benchmark.frame>=benchmark.frames && (benchmark.audioFrames==0 || benchmark.audioDone)){benchmark.recordLoop(publicationEnd-start-frameNanos,publishEnd-publicationEnd,System.nanoTime()-publishEnd,0,0,System.nanoTime()-start);gamePaused=true;continue}
                val measureEnd=if(benchmark!=null)System.nanoTime() else 0L
                var saveTime=0L
                var diagnosticsTime=0L
                count++
                if (count >= 120) {
                    val elapsed = System.nanoTime() - sampleStart
                    performance = "Game loop: %.1f FPS\nSimulation: %.2f ms/frame\nSpeed: %dx\nAudio underruns: %d\n".format(
                        count * 1e9 / elapsed, simulationNanos / 1e6 / count / multiplier,
                        multiplier, track?.underrunCount ?: 0)
                    val saveStart=if(benchmark!=null)System.nanoTime() else 0L
                    NativeBridge.gbaSave()
                    val saveEnd=if(benchmark!=null)System.nanoTime() else 0L
                    saveTime=saveEnd-saveStart
                    if(BuildConfig.DEBUG && benchmark!=null) {
                        java.io.File(context.cacheDir,"performance.txt").writeText(NativeBridge.gbaDiagnostics()+performance)
                    }
                    if(benchmark!=null)diagnosticsTime=System.nanoTime()-saveEnd
                    count = 0; simulationNanos = 0; sampleStart = System.nanoTime()
                }
                benchmark?.recordLoop(publicationEnd-start-frameNanos,publishEnd-publicationEnd,measureEnd-publishEnd,saveTime,diagnosticsTime,System.nanoTime()-start)
                // Actual GBA cadence: 280896 cycles / 16777216 Hz = 59.7275 FPS.
                if(benchmark!=null && !benchmark.paced)continue
                deadline += 16_742_706L
                val now = System.nanoTime()
                if (now - deadline > 4 * 16_742_706L) deadline = now
                while (running && System.nanoTime() < deadline) {
                    LockSupport.parkNanos((deadline-System.nanoTime()).coerceAtMost(2_000_000L))
                }
            }
        } catch (error: Throwable) {
            gamePaused = true
            post { onError(error.stackTraceToString()) }
        } finally {
            normalOutput.close()
            NativeBridge.gbaNormalAudioReader(false)
            try { NativeBridge.gbaCheckpoint() } catch (_: Throwable) {}
            try { track?.pause(); track?.flush(); track?.release() } catch (_: Exception) {}
        }
    }
}

private class UiBuffers(bytes:Int){
    val pixels:ByteBuffer=ByteBuffer.allocateDirect(480*160*4*2).order(ByteOrder.nativeOrder())
    val params=FloatArray(UI_PARAMS)
    var background:ByteBuffer=ByteBuffer.allocateDirect(bytes).order(ByteOrder.nativeOrder())
        private set
    fun resize(bytes:Int){if(background.capacity()!=bytes)background=ByteBuffer.allocateDirect(bytes).order(ByteOrder.nativeOrder())}
}

private class FrameRenderer(private val onError: (String) -> Unit) : GLSurfaceView.Renderer {
    @Volatile var displayedView=ViewConfig(0,0)
        private set
    private data class PublishedFrame(val pixels:ByteBuffer,val view:ViewConfig,val ui:UiBuffers)
    private val mailbox=FrameMailbox<PublishedFrame>()
    private var uploadedView:ViewConfig?=null
    private val uploadedUiParams=FloatArray(UI_PARAMS)
    @Volatile var screenWidth = 2280
        private set
    @Volatile var screenHeight = 1080
        private set
    private var textureWidth = 0
    private var textureHeight = 0
    private var texture = 0
    private var program = 0
    private var position = 0
    private var coordinates = 0
    private var uiProgram=0
    private var backdropProgram=0
    private val bandUv=ByteBuffer.allocateDirect(8*4).order(ByteOrder.nativeOrder()).asFloatBuffer()
    private var uiPosition=0
    private var uiCoordinates=0
    private val uiTextures=IntArray(3)
    private var bgWidth=0
    private var bgHeight=0
    private val vertices = floats(floatArrayOf(-1f,-1f, 1f,-1f, -1f,1f, 1f,1f))
    private val uv = floats(floatArrayOf(0f,1f, 1f,1f, 0f,0f, 1f,0f))

    fun publish(buffer:ByteBuffer,view:ViewConfig,ui:UiBuffers):Pair<ByteBuffer?,UiBuffers?> {
        val old=mailbox.publish(PublishedFrame(buffer,view,ui))
        return old?.pixels to old?.ui
    }

    override fun onSurfaceCreated(gl: GL10?, egl: EGLConfig?) {
        try {
            val vertex = shader(GL_VERTEX_SHADER,
                "attribute vec2 position; attribute vec2 uv; varying vec2 tex; void main(){ gl_Position=vec4(position,0.0,1.0);tex=uv; }")
            val fragment = shader(GL_FRAGMENT_SHADER,
                "precision mediump float; varying vec2 tex; uniform sampler2D image; void main(){ gl_FragColor=texture2D(image,tex).bgra; }")
            program = glCreateProgram()
            glAttachShader(program,vertex); glAttachShader(program,fragment); glLinkProgram(program)
            val linked = IntArray(1); glGetProgramiv(program,GL_LINK_STATUS,linked,0)
            check(linked[0] != 0) { glGetProgramInfoLog(program) }
            glDeleteShader(vertex); glDeleteShader(fragment)
            position = glGetAttribLocation(program,"position")
            coordinates = glGetAttribLocation(program,"uv")
            val textures = IntArray(1); glGenTextures(1,textures,0); texture = textures[0]
            glBindTexture(GL_TEXTURE_2D,texture)
            glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_NEAREST)
            glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_NEAREST)
            glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_S,GL_CLAMP_TO_EDGE)
            glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_T,GL_CLAMP_TO_EDGE)
            textureWidth = 0; textureHeight = 0
        uploadedView=null
            uiProgram=glCreateProgram()
            glAttachShader(uiProgram,shader(GL_VERTEX_SHADER,"""
                attribute vec2 position; attribute vec2 uv; varying vec2 tex;
                uniform vec4 rect; uniform vec2 screen;
                void main(){vec2 p=vec2(position.x*.5+.5,.5-position.y*.5)*rect.zw+rect.xy;
                  gl_Position=vec4(p.x/screen.x*2.-1.,1.-p.y/screen.y*2.,0.,1.);tex=uv;}
            """.trimIndent()))
            glAttachShader(uiProgram,shader(GL_FRAGMENT_SHADER,"""
                precision highp float;
                varying vec2 tex; uniform sampler2D panel; uniform sampler2D world;
                uniform vec2 screen; uniform vec4 worldRect; uniform vec4 clip;
                void main(){vec2 p=vec2(gl_FragCoord.x,screen.y-gl_FragCoord.y);
                  if(p.x<clip.x||p.y<clip.y||p.x>=clip.x+clip.z||p.y>=clip.y+clip.w)discard;
                  vec4 data=floor(texture2D(panel,tex)*255.+.5);
                  if(data.b==0.&&data.a==16.)discard;
                  float code=data.r+data.g*256.;
                  vec3 fg=vec3(mod(code,32.),mod(floor(code/32.),32.),mod(floor(code/1024.),32.));
                  vec3 bg=floor(texture2D(world,(p-worldRect.xy)/worldRect.zw).bgra.rgb*31.+.5);
                  vec3 lane=vec3(1.,2.,1.);
                  vec3 color=floor(min(floor((fg*lane*data.b+bg*lane*data.a+8.)/16.),vec3(31.,63.,31.))/lane);
                  gl_FragColor=vec4(color/31.,1.);
                }
            """.trimIndent()))
            glLinkProgram(uiProgram)
            glGetProgramiv(uiProgram,GL_LINK_STATUS,linked,0)
            check(linked[0]!=0){glGetProgramInfoLog(uiProgram)}
            uiPosition=glGetAttribLocation(uiProgram,"position");uiCoordinates=glGetAttribLocation(uiProgram,"uv")
            glGenTextures(3,uiTextures,0)
            for(i in 0..2){
                glBindTexture(GL_TEXTURE_2D,uiTextures[i])
                glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_NEAREST)
                glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_NEAREST)
                glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_S,GL_CLAMP_TO_EDGE)
                glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_T,GL_CLAMP_TO_EDGE)
                if(i<2)glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA,480,160,0,GL_RGBA,GL_UNSIGNED_BYTE,null)
            }
            bgWidth=0;bgHeight=0
            // Padded scenes: a blurred, dimmed copy of the 240x160 picture fills the bars.
            backdropProgram=glCreateProgram()
            glAttachShader(backdropProgram,shader(GL_VERTEX_SHADER,
                "attribute vec2 position; attribute vec2 uv; varying vec2 tex; void main(){ gl_Position=vec4(position,0.0,1.0);tex=uv; }"))
            glAttachShader(backdropProgram,shader(GL_FRAGMENT_SHADER,"""
                precision mediump float; varying vec2 tex; uniform sampler2D image; uniform vec4 region; uniform vec2 texel;
                void main(){vec2 p=region.xy+tex*region.zw;vec3 c=vec3(0.);
                  for(int i=-1;i<=1;i++)for(int j=-1;j<=1;j++)c+=texture2D(image,p+vec2(float(i),float(j))*texel).bgr;
                  gl_FragColor=vec4(c*(0.34/9.)+vec3(0.012,0.018,0.016),1.);}
            """.trimIndent()))
            glLinkProgram(backdropProgram)
            glGetProgramiv(backdropProgram,GL_LINK_STATUS,linked,0)
            check(linked[0]!=0){glGetProgramInfoLog(backdropProgram)}
        } catch (error: Throwable) { onError("OpenGL: ${error.message}") }
    }

    override fun onSurfaceChanged(gl: GL10?, width: Int, height: Int) {
        screenWidth = width; screenHeight = height
    }

    override fun onDrawFrame(gl: GL10?) {
        glViewport(0,0,screenWidth,screenHeight)
        glClearColor(0.025f,0.035f,0.03f,1f); glClear(GL_COLOR_BUFFER_BIT)
        if (program == 0) return
        val pending=mailbox.acquire()
        if(pending!=null)try {
            val buffer=pending.pixels
            val config=pending.view
            buffer.position(0)
            glActiveTexture(GL_TEXTURE0); glBindTexture(GL_TEXTURE_2D,texture)
            if (textureWidth != config.width || textureHeight != config.height) {
                glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA,config.width,config.height,0,GL_RGBA,GL_UNSIGNED_BYTE,buffer)
                textureWidth = config.width; textureHeight = config.height
            } else {
                glTexSubImage2D(GL_TEXTURE_2D,0,0,0,config.width,config.height,GL_RGBA,GL_UNSIGNED_BYTE,buffer)
            }
            pending.ui.let{ui->
                ui.params.copyInto(uploadedUiParams)
                val uiParams=uploadedUiParams
                for(layer in 0..1)if((0 until UI_SLOTS).any{uiParams[it*9+2]>0f && uiParams[it*9+8].toInt()%2==layer}){
                    ui.pixels.position(layer*480*160*4)
                    glBindTexture(GL_TEXTURE_2D,uiTextures[layer])
                    glTexSubImage2D(GL_TEXTURE_2D,0,0,0,480,160,GL_RGBA,GL_UNSIGNED_BYTE,ui.pixels)
                }
                if((0 until UI_SLOTS).any{uiParams[it*9+2]>0f && uiParams[it*9+8]>=2f}){
                    ui.background.position(0)
                    glBindTexture(GL_TEXTURE_2D,uiTextures[2])
                    if(bgWidth!=config.width||bgHeight!=config.height){
                        glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA,config.width,config.height,0,GL_RGBA,GL_UNSIGNED_BYTE,ui.background)
                        bgWidth=config.width;bgHeight=config.height
                    }else glTexSubImage2D(GL_TEXTURE_2D,0,0,0,config.width,config.height,GL_RGBA,GL_UNSIGNED_BYTE,ui.background)
                }
            }
            uploadedView=config
        } finally {
            mailbox.release(pending)
        }
        val view=uploadedView ?: return
        val uiParams=uploadedUiParams
        // Portrait shows only the native 240 columns across the full width (Engine::set_view).
        val portraitCrop=view.fillScreen && screenHeight>screenWidth && view.extraX<=8
        val scale = if(portraitCrop)screenWidth/240f
            else if(view.fillScreen)maxOf(screenWidth.toFloat()/view.width, screenHeight.toFloat()/view.height)
            else minOf(screenWidth.toFloat()/view.width, screenHeight.toFloat()/view.height)
        var width = (view.width*scale).toInt(); var height=(view.height*scale).toInt()
        var left=(screenWidth-width)/2; var bottom=(screenHeight-height)/2
        glActiveTexture(GL_TEXTURE0);glBindTexture(GL_TEXTURE_2D,texture)
        val padded=uiParams[UI_SLOTS*9]>0f && view.extraY>0 && backdropProgram!=0
        // Portrait/square views extend only 8 columns, which hold wrapped map leftovers outside
        // battles; wider landscape views keep their real widescreen columns.
        val cropX=view.extraX<=8
        val bandX=if(cropX)view.extraX else 0; val bandW=if(cropX)240 else view.width
        if(padded&&cropX){
            // Fit the 240x160 picture itself to the screen; Engine::set_view maps taps the same way.
            val fit=minOf(screenWidth/240f,screenHeight/160f)
            val bandLeft=((screenWidth-240*fit)/2).toInt(); val bandBottom=((screenHeight-160*fit)/2).toInt()
            width=(view.width*fit).toInt(); height=(view.height*fit).toInt()
            left=bandLeft-(view.extraX*fit).toInt(); bottom=bandBottom-(view.extraY*fit).toInt()
        }
        val bandScale=if(padded&&cropX)minOf(screenWidth/240f,screenHeight/160f) else scale
        val u0=bandX.toFloat()/view.width; val u1=(bandX+bandW).toFloat()/view.width
        val v0=view.extraY.toFloat()/view.height; val v1=(view.extraY+160f)/view.height
        if(padded){
            // Cover the screen with the band's central part, linearly filtered and blurred.
            val aspect=screenWidth.toFloat()/screenHeight
            val bandWidth=if(aspect<1.5f)240f*aspect/1.5f else 240f
            val bandHeight=if(aspect<1.5f)160f else 240f/aspect
            glViewport(0,0,screenWidth,screenHeight)
            glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_LINEAR)
            glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_LINEAR)
            glUseProgram(backdropProgram)
            glUniform1i(glGetUniformLocation(backdropProgram,"image"),0)
            if(view.extraY>=22){
                // The engine's 30x20 grid of 8x8 averages at texel (1,1) (engine.rs backdrop_grid).
                glUniform4f(glGetUniformLocation(backdropProgram,"region"),
                    (1f+(30f-30f*bandWidth/240f)/2)/view.width,(1f+(20f-20f*bandHeight/160f)/2)/view.height,
                    30f*bandWidth/240f/view.width,20f*bandHeight/160f/view.height)
                glUniform2f(glGetUniformLocation(backdropProgram,"texel"),0.75f/view.width,0.75f/view.height)
            }else{
                glUniform4f(glGetUniformLocation(backdropProgram,"region"),
                    (view.extraX+(240f-bandWidth)/2)/view.width,(view.extraY+(160f-bandHeight)/2)/view.height,
                    bandWidth/view.width,bandHeight/view.height)
                glUniform2f(glGetUniformLocation(backdropProgram,"texel"),4f/view.width,4f/view.height)
            }
            val bp=glGetAttribLocation(backdropProgram,"position");val bc=glGetAttribLocation(backdropProgram,"uv")
            glEnableVertexAttribArray(bp);glEnableVertexAttribArray(bc)
            vertices.position(0);uv.position(0)
            glVertexAttribPointer(bp,2,GL_FLOAT,false,0,vertices)
            glVertexAttribPointer(bc,2,GL_FLOAT,false,0,uv)
            glDrawArrays(GL_TRIANGLE_STRIP,0,4)
            glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_NEAREST)
            glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_NEAREST)
            // Then only the native 240x160 picture, without the padding rows or wrapped side columns.
            glViewport(left+(bandX*bandScale).toInt(),bottom+(view.extraY*bandScale).toInt(),(bandW*bandScale).toInt(),(160*bandScale).toInt())
            bandUv.position(0);bandUv.put(floatArrayOf(u0,v1, u1,v1, u0,v0, u1,v0));bandUv.position(0)
        }else glViewport(left,bottom,width,height)
        glUseProgram(program); glUniform1i(glGetUniformLocation(program,"image"),0)
        glEnableVertexAttribArray(position); glEnableVertexAttribArray(coordinates)
        vertices.position(0); uv.position(0)
        glVertexAttribPointer(position,2,GL_FLOAT,false,0,vertices)
        glVertexAttribPointer(coordinates,2,GL_FLOAT,false,0,if(padded)bandUv else uv)
        glDrawArrays(GL_TRIANGLE_STRIP,0,4)
        if(uiProgram!=0){
            glViewport(0,0,screenWidth,screenHeight)
            glUseProgram(uiProgram)
            glUniform2f(glGetUniformLocation(uiProgram,"screen"),screenWidth.toFloat(),screenHeight.toFloat())
            glUniform4f(glGetUniformLocation(uiProgram,"worldRect"),left.toFloat(),
                (screenHeight-bottom-height).toFloat(),width.toFloat(),height.toFloat())
            glUniform1i(glGetUniformLocation(uiProgram,"world"),0)
            glUniform1i(glGetUniformLocation(uiProgram,"panel"),1)
            glEnableVertexAttribArray(uiPosition);glEnableVertexAttribArray(uiCoordinates)
            vertices.position(0);uv.position(0)
            glVertexAttribPointer(uiPosition,2,GL_FLOAT,false,0,vertices)
            glVertexAttribPointer(uiCoordinates,2,GL_FLOAT,false,0,uv)
            // Each slot is one clipped piece of UI layer (meta % 2); meta >= 2 blends over the stats background.
            for(i in 0 until UI_SLOTS)if(uiParams[i*9+2]>0f){
                glActiveTexture(GL_TEXTURE0);glBindTexture(GL_TEXTURE_2D,if(uiParams[i*9+8]>=2f)uiTextures[2]else texture)
                glActiveTexture(GL_TEXTURE1);glBindTexture(GL_TEXTURE_2D,uiTextures[uiParams[i*9+8].toInt()%2])
                glUniform4fv(glGetUniformLocation(uiProgram,"rect"),1,uiParams,i*9)
                glUniform4fv(glGetUniformLocation(uiProgram,"clip"),1,uiParams,i*9+4)
                glDrawArrays(GL_TRIANGLE_STRIP,0,4)
            }
            glActiveTexture(GL_TEXTURE0)
        }
        displayedView=view
    }
    private fun shader(type: Int, source: String): Int {
        val shader = glCreateShader(type); glShaderSource(shader,source); glCompileShader(shader)
        val status=IntArray(1);glGetShaderiv(shader,GL_COMPILE_STATUS,status,0)
        check(status[0] != 0) { glGetShaderInfoLog(shader) }
        return shader
    }
    private fun floats(values: FloatArray): FloatBuffer = ByteBuffer.allocateDirect(values.size*4)
        .order(ByteOrder.nativeOrder()).asFloatBuffer().apply { put(values);position(0) }
}
