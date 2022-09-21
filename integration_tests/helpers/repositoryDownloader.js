const shell = require('shelljs')

function downloadRepository(url, destPath) {
    //TODO: we don't need to download the repo if it has been already downloaded

    shell.cd(destPath)
    shell.exec('git clone https://github.com/tari-project/tari')
}

module.exports = downloadRepository;
