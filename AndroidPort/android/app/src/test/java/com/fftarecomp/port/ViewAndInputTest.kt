package com.fftarecomp.port

import org.junit.Assert.*
import org.junit.Test

class ViewAndInputTest {
    @Test fun zoomExpandsWorldInBothAxesWithinCoreLimits() {
        AspectMode.entries.forEach { aspect ->
            val base=ViewConfig.create(aspect,0)
            assertEquals(aspect.baseHeight,base.height)
            assertEquals(240+2*aspect.baseExtra,base.width)
            val expanded=ViewConfig.create(aspect,1)
            assertTrue(expanded.width>base.width)
            assertTrue(expanded.height>base.height)
            assertTrue(ViewConfig.maxZoom(aspect)<=6)
            assertEquals(ViewConfig.create(aspect,6),ViewConfig.create(aspect,1000))
            val maximum=ViewConfig.create(aspect,1000)
            assertTrue(maximum.width<=1536)
            assertTrue(maximum.height<=800)
        }
    }
    @Test fun squareAndPhoneModesUseTheirActualDimensions() {
        val square=ViewConfig.create(AspectMode.SQUARE,0)
        assertEquals(square.width,square.height)
        val phone=ViewConfig.create(AspectMode.DEVICE,0,2280,1080)
        assertEquals(338,phone.width)
        assertEquals(160,phone.height)
        assertTrue(phone.fillScreen)
    }
    @Test fun releasesDoNotCancelOtherInputSources() {
        val input=GameInput()
        input.set("touch",1)
        input.set("controller",1 or 64)
        input.set("touch",0)
        assertEquals(65,input.buttons.get())
        input.clear()
        assertEquals(0,input.buttons.get())
    }
    @Test fun portraitAlwaysUsesTheFullTallScreen() {
        val request=ViewRequest(AspectMode.WIDESCREEN,0)
        assertEquals(AspectMode.WIDESCREEN,request.aspectFor(2400,1080))
        assertEquals(AspectMode.DEVICE,request.aspectFor(1080,2400))
        val view=ViewConfig.create(request.aspectFor(1080,2400),0,1080,2400)
        assertEquals(240,view.width)
        assertTrue(view.height>500)
        assertTrue(view.fillScreen)
    }
}
