package com.niklashanft.echo.client.audio.speaker;

import com.niklashanft.echo.client.util.EchoAudioFormat;

import javax.sound.sampled.*;
import java.util.UUID;

public class Speaker extends Thread {

    private final UUID channelId;
    private SourceDataLine speaker;

    public Speaker(UUID channelId) {
        this.channelId = channelId;

        AudioFormat af = EchoAudioFormat.ECHO_AUDIO_FORMAT;
        DataLine.Info info = new DataLine.Info(SourceDataLine.class, af);

        try {
            speaker = (SourceDataLine) AudioSystem.getLine(info);
            speaker.open(af);
            speaker.start();
            //GainControl.INSTANCE.setLine(speaker);
        } catch (LineUnavailableException e) {
            e.printStackTrace();
        }
    }

    public void play(byte[] bytes) {
        this.speaker.write(bytes, 0, bytes.length);
    }

    public void shutdown() {
        this.speaker.stop();
        this.speaker.flush();
        this.speaker.close();
    }

    public UUID getChannelId() {
        return this.channelId;
    }

}
