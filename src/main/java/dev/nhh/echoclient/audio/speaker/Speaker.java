package dev.nhh.echoclient.audio.speaker;

import javax.sound.sampled.AudioFormat;
import javax.sound.sampled.AudioSystem;
import javax.sound.sampled.DataLine.Info;
import javax.sound.sampled.LineUnavailableException;
import javax.sound.sampled.SourceDataLine;

public enum Speaker {

    INSTANCE;

    private SourceDataLine speakers;
    private final AudioFormat format = new AudioFormat(16000, 16, 1, true, true);
    private final Info info = new Info(SourceDataLine.class, format);

    Speaker() {
        try {
            speakers = (SourceDataLine) AudioSystem.getLine(info);
            speakers.open(format);
            speakers.start();
        } catch (LineUnavailableException e) {
            e.printStackTrace();
        }
    }

    public void write(AudioPacket packet) {
        this.speakers.write(packet.getBytes(), packet.getOffset(), packet.getLength());
    }

}
