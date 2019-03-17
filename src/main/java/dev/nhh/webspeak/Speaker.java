package dev.nhh.webspeak;

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
    private boolean started = false;

    public void start() {
        if(started) return;
        try {
            speakers = (SourceDataLine) AudioSystem.getLine(info);
            speakers.open(format);
            speakers.start();
            started = true;
        } catch (LineUnavailableException e) {
            e.printStackTrace();
        }
    }

    public void write(byte[] bytes, int off, int length) {
        this.speakers.write(bytes, off, length);
    }

}
