package dev.nhh.echo;

import dev.nhh.echo.audio.speaker.SpeakerThread;
import dev.nhh.echo.client.Client;

public class WebspeakClient {

    public static void main(String[] args) {
        final var clientThread = new Thread(new Client("localhost"));
        clientThread.start();

        final var speakerThread = new Thread(new SpeakerThread());
        speakerThread.start();
    }

}
