package com.fftarecomp.port
import org.junit.Assert.*
import org.junit.Test

class TouchLayoutTest {
    @Test fun draggingIsResolutionIndependentAndKeepsControlsOnScreen(){
        val start=TouchPlacement(.5f,.5f)
        assertEquals(TouchPlacement(.6f,.7f),start.move(100f,100f,1000f,500f))
        assertEquals(start.move(100f,100f,1000f,500f),start.move(200f,200f,2000f,1000f))
        assertEquals(TouchPlacement(1f,0f),start.move(99999f,-99999f,1000f,500f))
        assertEquals(start,start.move(100f,100f,0f,0f))
    }
    @Test fun cancelRestoresPositionsAndHiddenButtonsAndDefaultRestoresInitialLayout(){
        val layout=TouchPositions()
        val original=layout.snapshot()
        layout.positions[TouchControl.A]=TouchPlacement(.1f,.2f,false)
        assertNotEquals(original,layout.snapshot())
        layout.restore(original)
        assertEquals(original,layout.snapshot())
        layout.positions[TouchControl.DPAD]=TouchPlacement(.5f,.2f,false)
        layout.reset()
        assertEquals(original,layout.snapshot())
        assertFalse(layout.positions.getValue(TouchControl.PAUSE).visible)
        assertFalse(layout.positions.getValue(TouchControl.FAST_HOLD).visible)
    }
    @Test fun portraitDefaultsKeepGameButtonsInTheLowerScreenAndResetToThemselves(){
        val layout=TouchPositions(::portraitDefault)
        for(control in listOf(TouchControl.L,TouchControl.R,TouchControl.A,TouchControl.B,TouchControl.DPAD))
            assertTrue(control.name,layout.positions.getValue(control).y>=.65f)
        layout.positions[TouchControl.A]=TouchPlacement(.1f,.1f)
        layout.reset()
        assertEquals(portraitDefault(TouchControl.A),layout.positions.getValue(TouchControl.A))
        assertNotEquals(landscapeDefault(TouchControl.L),portraitDefault(TouchControl.L))
    }
    @Test fun invalidStoredCoordinatesCannotMakeAControlUnreachable(){
        assertEquals(TouchPlacement(0f,1f),TouchPlacement(Float.NaN,2f).bounded())
    }

    @org.junit.Test fun individualSizesAreBoundedAndSurviveCancelSnapshot(){
        val layout=TouchPositions();val original=layout.snapshot()
        layout.positions[TouchControl.L]=layout.positions.getValue(TouchControl.L).copy(scale=1.7f)
        assertEquals(1.7f,layout.positions.getValue(TouchControl.L).scale)
        layout.restore(original);assertEquals(1f,layout.positions.getValue(TouchControl.L).scale)
        assertEquals(2f,TouchPlacement(0f,0f,scale=9f).bounded().scale)
        assertEquals(1f,TouchPlacement(0f,0f,scale=Float.NaN).bounded().scale)
    }
    @org.junit.Test fun bottomRowStaysCenteredWithEqualGapsOnDifferentScreens(){
        for(width in listOf(360f,480f,860f)){
            val widths=listOf(66f,66f,66f)
            val left=widths.indices.map{TouchGeometry.rowLeft(it,width,widths,12f)}
            assertEquals(12f,left[1]-left[0]-widths[0],.001f)
            assertEquals(12f,left[2]-left[1]-widths[1],.001f)
            assertEquals(width/2,(left.first()+left.last()+widths.last())/2,.001f)
        }
    }
}
