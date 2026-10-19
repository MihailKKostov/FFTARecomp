package com.fftarecomp.port

data class ScreenTap(val x:Float,val y:Float,val back:Boolean=false)

/** Inverse of the actual GL viewport, including letterboxing and full-screen crop. */
object TouchSelection {
    fun map(x:Float,y:Float,width:Int,height:Int,view:ViewConfig):Pair<Float,Float>? {
        if(width<=0 || height<=0 || !x.isFinite() || !y.isFinite())return null
        val scale=if(view.fillScreen && height>width && view.extraX<=8)width/240f
            else if(view.fillScreen)maxOf(width.toFloat()/view.width,height.toFloat()/view.height)
            else minOf(width.toFloat()/view.width,height.toFloat()/view.height)
        val w=(view.width*scale).toInt();val h=(view.height*scale).toInt()
        val nx=(x-(width-w)/2)/w;val ny=(y-(height-h)/2)/h
        return if(nx>=0f && nx<1f && ny>=0f && ny<1f)nx to ny else null
    }
    /** GBA direction mask for a drag offset: the dominant axis only, nothing inside the dead zone. */
    fun direction(dx:Float,dy:Float,deadZone:Float):Int {
        if(!dx.isFinite() || !dy.isFinite() || dx*dx+dy*dy<deadZone*deadZone)return 0
        return if(kotlin.math.abs(dx)>=kotlin.math.abs(dy)){if(dx>0f)16 else 32}else{if(dy>0f)128 else 64}
    }
}
