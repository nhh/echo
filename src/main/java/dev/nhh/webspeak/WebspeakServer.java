package dev.nhh.webspeak;

public class WebspeakServer {

    public static void main(String[] args) {

        var server = new Server();
        var serverThread = new Thread(server);
        serverThread.start();

    }

}
