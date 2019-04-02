package dev.nhh.echoclient.controller;

import dev.nhh.echoclient.audio.microphone.MicrophoneThreadWrapper;
import dev.nhh.echoclient.audio.speaker.SpeakerThreadWrapper;
import dev.nhh.echoclient.network.AudioConnectionThreadWrapper;
import dev.nhh.echoclient.util.ThreadScheduler;
import javafx.fxml.FXML;
import javafx.scene.control.TextField;
import javafx.scene.input.MouseEvent;
import javafx.stage.Window;

public class LayoutController {

    @FXML
    private TextField textField;

    @FXML
    private Window dialog;

    @FXML
    protected void startMicrophone(MouseEvent event) {
        ThreadScheduler.INSTANCE.addWrapper(new MicrophoneThreadWrapper("microphoneThreadWrapper", true));
        ThreadScheduler.INSTANCE.start("microphoneThreadWrapper");
    }

    @FXML
    protected void stopMicrophone(MouseEvent event) {
        ThreadScheduler.INSTANCE.stop("microphoneThreadWrapper");
    }

    @FXML
    protected void start(MouseEvent event) {
        ThreadScheduler.INSTANCE.addWrapper(new SpeakerThreadWrapper("speakerThreadWrapper", true));
        ThreadScheduler.INSTANCE.start("speakerThreadWrapper");
    }

    @FXML
    protected void stop(MouseEvent event) {
        ThreadScheduler.INSTANCE.stop("speakerThreadWrapper");
    }

    @FXML
    protected void startAudioConnection(MouseEvent event) {
        ThreadScheduler.INSTANCE.addWrapper(new AudioConnectionThreadWrapper("audioConnectionThreadWrapper", true));
        ThreadScheduler.INSTANCE.start("audioConnectionThreadWrapper");
    }

    @FXML
    protected void stopAudioConnection(MouseEvent event) {
        ThreadScheduler.INSTANCE.stop("audioConnectionThreadWrapper");
    }

}
