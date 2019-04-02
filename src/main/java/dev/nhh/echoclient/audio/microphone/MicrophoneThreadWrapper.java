package dev.nhh.echoclient.audio.microphone;

import dev.nhh.echoclient.network.AudioConnectionQueue;
import dev.nhh.echoclient.util.ThreadWrapper;

public class MicrophoneThreadWrapper extends ThreadWrapper {

    private final AudioConnectionQueue audioConnectionQueue = AudioConnectionQueue.INSTANCE;
    private final Microphone microphone = Microphone.INSTANCE;

    public MicrophoneThreadWrapper(String name, boolean daemon) {
        super(name, daemon);
    }

    @Override
    public void run() {
        while(this.isRunning.get()) {
            microphone.read(0, 1024);
            this.audioConnectionQueue.add(new MicrophonePacket(microphone.getData(), microphone.getNumBytesRead()));
        }
    }

}
