<template>
    <div>
        <h1 class="red">Whats u asdp!  123213asdsadsda 231</h1>
        <div> Javascript {{ js }}</div>
        <div> Java {{ java }}</div>

        <button @click="doIt">Feini</button>
    </div>
</template>

<script>
    //import {kickMember} from "echo/client";

    export default  {
        data() {
            return {
                js: "",
                java: "",
            }
        },
        methods: {
            doIt() {
                window.myObject.doIt();
            }
        },
        mounted() {
            setInterval(() => this.js = Date.now())
            window.addEventListener("echo", () => this.java = Date.now())
        }
    }
</script>


<style>
html {
    width: 100%;
    height: 100%;
}
</style>
