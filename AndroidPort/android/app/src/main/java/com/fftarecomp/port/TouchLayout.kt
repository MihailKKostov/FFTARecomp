package com.fftarecomp.port

import android.content.Context
import androidx.compose.runtime.*

enum class TouchControl(val label:String,val width:Int,val height:Int,val x:Float,val y:Float,
    val mask:Int=0,val visibleByDefault:Boolean=true){
    PAUSE("Pause",86,40,0f,0f,visibleByDefault=false),
    FAST_TOGGLE("Toggle FF",66,36,.61f,1f),
    ZOOM_OUT("-",42,42,1f,.46135613f),ZOOM_IN("+",42,42,1f,.32546404f),
    L("L",84,42,0f,0f,512),R("R",84,42,1f,0f,256),
    DPAD("D-pad",140,140,0f,.98f),
    SELECT("Select",66,36,.39f,1f,4),START("Start",66,36,.50f,1f,8),
    B("B",58,58,.87f,.98f,2),A("A",64,64,.995271f,.8545261f,1),
    FAST_HOLD("Hold FF",72,40,.68f,0f,visibleByDefault=false)
}
data class TouchPlacement(val x:Float,val y:Float,val visible:Boolean=true,val scale:Float=1f,val centeredRow:Boolean=false){
    fun bounded()=copy(x=if(x.isFinite())x.coerceIn(0f,1f)else 0f,
        y=if(y.isFinite())y.coerceIn(0f,1f)else 0f,
        scale=if(scale.isFinite())scale.coerceIn(.5f,2f)else 1f)
    fun move(dx:Float,dy:Float,availableWidth:Float,availableHeight:Float)=copy(
        x=x+if(availableWidth>0f)dx/availableWidth else 0f,
        y=y+if(availableHeight>0f)dy/availableHeight else 0f,centeredRow=false).bounded()
}
class TouchPositions {
    val positions=mutableStateMapOf<TouchControl,TouchPlacement>()
    init{reset()}
    fun reset(){positions.clear();TouchControl.entries.forEach{positions[it]=TouchPlacement(it.x,it.y,it.visibleByDefault,centeredRow=it in TouchGeometry.bottomRow)}}
    fun snapshot():Map<TouchControl,TouchPlacement> = positions.toMap()
    fun restore(snapshot:Map<TouchControl,TouchPlacement>){
        reset();snapshot.forEach{(key,value)->positions[key]=value.bounded()}
    }
}
@Stable
class TouchLayout(context:Context){
    private val prefs=context.getSharedPreferences("touch_layout_v1",Context.MODE_PRIVATE)
    val layout=TouchPositions().apply{
        val alignSavedBottomRow=prefs.getInt("layout_revision",0)<1
        TouchControl.entries.forEach{control->
            val oldX=prefs.getFloat(control.name+"_x",control.x)
            val oldY=prefs.getFloat(control.name+"_y",control.y)
            val align=control in TouchGeometry.bottomRow
            positions[control]=TouchPlacement(
                if(alignSavedBottomRow&&align)control.x else oldX,
                if(alignSavedBottomRow&&align)1f else oldY,
                prefs.getBoolean(control.name+"_visible",control.visibleByDefault),
                prefs.getFloat(control.name+"_scale",1f),
                alignSavedBottomRow&&align || prefs.getBoolean(control.name+"_centered",align && !prefs.contains(control.name+"_x"))).bounded()
        }
    }
    var vibration by mutableStateOf(prefs.getBoolean("vibration",false))
    var experimentalClicks by mutableStateOf(prefs.getBoolean("experimental_clicks",false))
    /** No virtual gamepad: taps select, drags steer, two-finger tap goes back. */
    var touchOnly by mutableStateOf(prefs.getBoolean("touch_only",false))
    fun save(){
        prefs.edit().apply{
            putBoolean("vibration",vibration)
            putBoolean("experimental_clicks",experimentalClicks)
            putBoolean("touch_only",touchOnly)
            putInt("layout_revision",1)
            layout.positions.forEach{(control,pose)->
                putFloat(control.name+"_x",pose.x);putFloat(control.name+"_y",pose.y)
                putBoolean(control.name+"_visible",pose.visible)
                putFloat(control.name+"_scale",pose.scale);putBoolean(control.name+"_centered",pose.centeredRow)
            }
        }.apply()
    }
}

/** Equal physical gaps, independent of screen aspect ratio and button widths. */
object TouchGeometry {
    val bottomRow=listOf(TouchControl.SELECT,TouchControl.START,TouchControl.FAST_TOGGLE)
    fun rowLeft(index:Int,canvasWidth:Float,widths:List<Float>,gap:Float):Float {
        val total=widths.sum()+gap*(widths.size-1)
        return ((canvasWidth-total)/2+widths.take(index).sum()+gap*index)
            .coerceIn(0f,(canvasWidth-widths[index]).coerceAtLeast(0f))
    }
    fun rowTop(canvasHeight:Float,controlHeight:Float,anchorY:Float):Float =
        (anchorY*(canvasHeight-controlHeight).coerceAtLeast(0f)).coerceIn(0f,(canvasHeight-controlHeight).coerceAtLeast(0f))
    fun rowCenterY(canvasHeight:Float,anchorHeight:Float,anchorY:Float):Float =
        rowTop(canvasHeight,anchorHeight,anchorY)+anchorHeight/2f
    fun rowItemTop(canvasHeight:Float,itemHeight:Float,centerY:Float):Float =
        (centerY-itemHeight/2f).coerceIn(0f,(canvasHeight-itemHeight).coerceAtLeast(0f))
}
