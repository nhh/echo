package dev.nhh.webspeak;

public class WebspeakClient {

    public static void main(String[] args) {
        var client = new Client("localhost");
        var clientThread = new Thread(client);
        clientThread.start();
    }

}
