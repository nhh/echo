package dev.nhh.echo;

import dev.nhh.echo.audio.speaker.SpeakerThread;
import dev.nhh.echo.client.Client;

public class WebspeakClient {

    public static void main(String[] args) {

        // Todo Refactor Microphone into MicrophoneQueue and Thread so the client
        // can poll and sleep if there is no input
        
        final var clientThread = new Thread(new Client("localhost"));
        clientThread.setName("Client Thread");
        clientThread.start();

        final var speakerThread = new Thread(new SpeakerThread());
        speakerThread.setName("Speaker Queue Thread");
        speakerThread.start();
    }

}
