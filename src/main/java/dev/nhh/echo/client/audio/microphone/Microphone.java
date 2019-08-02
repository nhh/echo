package dev.nhh.echo.client.audio.microphone;

import dev.nhh.echo.client.dto.VoicePacket;
import dev.nhh.echo.client.util.EchoAudioFormat;

import javax.sound.sampled.*;
import java.io.IOException;
import java.io.ObjectOutputStream;
import java.util.concurrent.atomic.AtomicBoolean;

public class Microphone extends Thread {

    private TargetDataLine microphone;
    ObjectOutputStream serverStream;
    private AtomicBoolean isRunning = new AtomicBoolean(false);

    public Microphone(ObjectOutputStream stream) {
        this.serverStream = stream;
        AudioFormat af = EchoAudioFormat.EIGHT_BIT_PER_SECOND;
        DataLine.Info info = new DataLine.Info(TargetDataLine.class, null);

        try {
            microphone = (TargetDataLine) AudioSystem.getLine(info);
            microphone.open(af);
            microphone.start();
        } catch (LineUnavailableException e) {
            e.printStackTrace();
        }
    }

    @Override
    public void run() {
        if (this.isRunning.get()) {
            return;
        }

        this.isRunning.set(true);

        while(isRunning.get()) {
            if (this.microphone.available() <= 1200) { // We want to only play sound if enough is available - Only play if 1201 is available
                try { Thread.sleep(10); } catch (InterruptedException e) { e.printStackTrace(); }
                continue;
            }
            byte[] buff = new byte[1200];
            microphone.read(buff, 0, buff.length);

            var p = new VoicePacket(null, null, -1, buff); // Todo apply Gzip compression

            try {
                this.serverStream.writeObject(p);
            } catch (IOException e) {
                e.printStackTrace();
            }
        }

    }

    public void close() {
        if(!this.isRunning.get()) return;
        this.isRunning.set(false);
        this.microphone.stop();
        this.microphone.flush();
        this.microphone.close();
    }


}
