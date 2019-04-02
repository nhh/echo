package dev.nhh.echoclient.audio.microphone;

public class MicrophonePacket {

    private final byte[] data;
    private final int bytesRead;

    public MicrophonePacket(byte[] data, int bytesRead) {
        this.data = data;
        this.bytesRead = bytesRead;
    }

    public byte[] getData() {
        return data;
    }

    public int getBytesRead() {
        return bytesRead;
    }
}
