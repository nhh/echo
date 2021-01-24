package com.niklashanft.echo.client.audio.speaker;

import javax.sound.sampled.FloatControl;
import javax.sound.sampled.SourceDataLine;

public enum GainControl {
    INSTANCE;

    FloatControl control = null;

    public void setVolume(float volume) {
        if(this.control == null) return;
        if(volume >= 99.9) return;
        this.control.setValue(volume);
    }

    public void setLine(SourceDataLine line) {
        this.control = (FloatControl) line.getControl(FloatControl.Type.MASTER_GAIN);
    }

}
