package dev.nhh.echo.client.audio.microphone;

import dev.nhh.echo.client.Client;
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
            if (microphone.available() <= 1100) {
                continue;
            }

            byte[] buff = new byte[2200];
            microphone.read(buff, 0, buff.length);

            VoicePacket p = new VoicePacket(null, Client.CLIENT_ID, System.nanoTime() / 1000000L, buff); // Todo apply Gzip compression

            try {
                this.serverStream.writeObject(p);
            } catch (IOException e) {
                this.shutdown();
                e.printStackTrace();
            }
        }

        this.microphone.stop();
        this.microphone.flush();
        this.microphone.close();

    }

    public void shutdown() {
        if(!this.isRunning.get()) return;
        this.isRunning.set(false);

    }


}
