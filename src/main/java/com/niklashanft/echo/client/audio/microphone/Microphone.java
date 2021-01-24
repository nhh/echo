package com.niklashanft.echo.client.audio.microphone;

import com.niklashanft.echo.client.util.EchoAudioFormat;

import javax.sound.sampled.*;

public class Microphone {

    private TargetDataLine microphone;

    public Microphone() {
        AudioFormat af = EchoAudioFormat.ECHO_AUDIO_FORMAT;
        DataLine.Info info = new DataLine.Info(TargetDataLine.class, null);

        try {
            microphone = (TargetDataLine) AudioSystem.getLine(info);
            microphone.open(af);
        } catch (LineUnavailableException e) {
            e.printStackTrace();
        }
    }

    public byte[] read() {
        while(microphone.available() <= 2200) {} // Block until data is available

        byte[] buff = new byte[2200];
        microphone.read(buff, 0, buff.length);
        return buff;
    }

    public void stop() {
        this.microphone.flush();
        this.microphone.stop();
    }

    public void start() {
        this.microphone.flush();
        this.microphone.start();
    }

}
