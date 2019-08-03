package dev.nhh.echo.client.audio.speaker;

import dev.nhh.echo.client.dto.VoicePacket;
import dev.nhh.echo.client.util.EchoAudioFormat;

import javax.sound.sampled.*;
import java.util.ArrayList;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicBoolean;

public class AudioChannel extends Thread {
    private AtomicBoolean isRunning = new AtomicBoolean(false);

    private SourceDataLine speaker;
    private ArrayList<VoicePacket> voicePacketQueue = new ArrayList<>();
    private final UUID channelId;

    public AudioChannel(UUID channelId) {
        this.channelId = channelId;

        AudioFormat af = EchoAudioFormat.EIGHT_BIT_PER_SECOND;
        DataLine.Info info = new DataLine.Info(SourceDataLine.class, af);

        try {
            speaker = (SourceDataLine) AudioSystem.getLine(info);
            speaker.open(af);
            speaker.start();
        } catch (LineUnavailableException e) {
            e.printStackTrace();
        }
    }

    public void addVoicePacketToQueue(VoicePacket vp) {
        this.voicePacketQueue.add(vp);
    }

    @Override
    public void run() {
        if (this.isRunning.get()) {
            return;
        }

        this.isRunning.set(true);

        while(this.isRunning.get()) {
            //nothing to play, wait
            if (voicePacketQueue.isEmpty()) {
                continue;
            }

            VoicePacket vp = this.voicePacketQueue.get(0);
            voicePacketQueue.remove(vp);
            this.speaker.write(vp.getData(), 0, vp.getData().length);
        }

        this.speaker.stop();
        this.speaker.flush();
        this.speaker.close();

    }

    public void shutdown() {
        if(!this.isRunning.get()) return;
        this.isRunning.set(false);
    }

    public UUID getChannelId() {
        return channelId;
    }
}
