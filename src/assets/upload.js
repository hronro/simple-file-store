// Enable the styles that only applies when JavaScript is enabled.
document.body.classList.add('js-enabled')

// Check if the browser supports the Invoker Commands API.
// If not, we have to manually open the upload dialog 
// while the upload button is clicked.
if (!('command' in HTMLButtonElement.prototype &&
	'source' in ((window.CommandEvent || {}).prototype || {}))) {
	window['uploadBtn'].onclick = function openUploadDialog() {
		window['uploadDialog'].showModal()
	}
}

const uploadFormElement = document.getElementById('uploadForm')
const fileInputElement = document.getElementById('fileInput')
const inputGroupElement = document.getElementById('inputGroup')
const resumableUploadElement = document.getElementById('resumableUpload')
const uploadProgressContainerElement = document.getElementById('uploadProgressContainer')
const uploadProgressTextElement = document.getElementById('uploadProgressText')
const uploadProgressBarElement = document.getElementById('uploadProgressBar')
const uploadSubmitBtnElement = document.getElementById('uploadSubmitBtn')

// Enable selected file preview
fileInputElement.onchange = function handleFilePreview(event) {
	showSelectedFilePreview()
}
fileInputElement.oncancel = function handleFileCancel() {
	showSelectedFilePreview()
}
function showSelectedFilePreview() {
	const file = fileInputElement.files[0]
	let previewContainer = document.getElementById('selectedFilePreview')
	if (file == null) {
		if (previewContainer != null) {
			inputGroupElement.removeChild(previewContainer)
		}
	} else {
		const fileType = file.name.split('.').pop().toUpperCase()
		if (previewContainer != null) {
			previewContainer.querySelector('#selectedFileName').innerText = file.name
			previewContainer.querySelector('#selectedFileSize').innerText = prettyFileSize(file.size)
			previewContainer.querySelector('#selectedFileType').innerText = fileType
		} else {
			previewContainer = Object.assign(document.createElement('div'), {
				className: 'selected-file',
				id: 'selectedFilePreview',
			})
			previewContainer.innerHTML = `<div class="file-preview" id="selectedFileType">${fileType}</div>
					<div class="file-info">
						<div class="file-name" id="selectedFileName">${file.name}</div>
						<div class="file-size" id="selectedFileSize">${prettyFileSize(file.size)}</div>
					</div>`
			previewContainer.appendChild(
				Object.assign(document.createElement('button'), {
					type: 'button',
					className: 'remove-selected-file',
					innerText: '×',
					onclick: function removeSelectedFile() {
						fileInputElement.value = ''
						showSelectedFilePreview()
					},
				})
			)
			inputGroupElement.appendChild(previewContainer)
		}
	}
}

// Enable the resumue upload toggle, and make it on by default
resumableUploadElement.removeAttribute('disabled')
resumableUploadElement.checked = true

// Override the native form submit event,
// to make progress bar and resumable upload work.
uploadFormElement.onsubmit = function handleUploadFormSubmit(event) {
	event.preventDefault()

	if (resumableUploadElement.checked) {
		resumableUpload()
	} else {
		normalUpload()
	}

	// Disable the upload button and the resumable toggle before the upload finishes
	uploadSubmitBtnElement.disabled = true
	resumableUploadElement.disabled = true
	fileInputElement.disabled = true
}

function normalUpload() {
	const formData = new FormData(uploadFormElement)

	const xhr = new XMLHttpRequest()

	let uploadProgress = 0
	let stopProgressBarAnimation = null

	xhr.upload.onloadstart = function handleUplaodStart() {
		showUploadProgress()
		stopProgressBarAnimation = startProgressBarAnimation({
			getProgress() {
				return uploadProgress
			},
		})
	}
	xhr.upload.onprogress = function handleUploadProgress(event) {
		uploadProgress = event.loaded / event.total
	}

	function handleUploadEnd() {
		if (stopProgressBarAnimation != null) {
			stopProgressBarAnimation()
			stopProgressBarAnimation = null
		}
	}
	xhr.upload.onloadend = handleUploadEnd
	xhr.upload.onerror = handleUploadEnd
	xhr.upload.onabort = handleUploadEnd
	xhr.upload.ontimeout = handleUploadEnd

	xhr.onreadystatechange = function handleXhrEnd() {
		if (xhr.readyState === XMLHttpRequest.DONE) {
			if (xhr.status === 200) {
				alert('Upload complete!')
				window.location.reload()
			} else {
				alert('Upload failed: ' + xhr.statusText)
			}
		}
	}

	xhr.open('POST', uploadFormElement.action, true)
	xhr.send(formData)
}

async function resumableUpload() {
	const uriPrefix = uploadFormElement.getAttribute('action').replace('/files/', '/upload/')
	/** @type File */
	const file = fileInputElement.files[0]

	let uploadProgress = 0
	let stopProgressBarAnimation = startProgressBarAnimation({
		getProgress() {
			return uploadProgress
		},
	})
	showUploadProgress()

	/**
	 * @type {{ chunkSize: number; fileSize: number; chunks: Record<number, (0 | 1 | 2)> }}
	 */
	let meta

	const getMetaResponse = await fetch(`${uriPrefix}${file.name}`)
	if (getMetaResponse.ok) {
		meta = await getMetaResponse.json()
	} else {
		meta = await (await fetch(`${uriPrefix}${file.name}`, {
			method: 'POST',
			headers: {
				'Content-Type': 'application/json',
			},
			body: JSON.stringify({
				size: file.size,
			}),
		})).json()
	}

	const totalChunks = Object.keys(meta.chunks).length
	let completedChunks = Object.values(meta.chunks).filter(status => status === 2).length
	uploadProgress = completedChunks / totalChunks

	// TODO: We should ask the server to mark all ongoing chunks as not started before we start uploading,
	// for now let's just assume all chunks that are not completed are all not started.

	const unuploadedChunkIndexes = Object.entries(meta.chunks).filter(([_, status]) => status !== 2).map(([index, _]) => parseInt(index, 10)).reverse()
	await makePromisePool(4, () => {
		const chunkIndex = unuploadedChunkIndexes.pop()

		if (chunkIndex == null) {
			return null
		}

		const chunkedFile = file.slice(chunkIndex * meta.chunkSize, (chunkIndex + 1) * meta.chunkSize)
		return (async function uploadChunk() {
			const data = await chunkedFile.arrayBuffer()

			let retryTimes = 3

			while (retryTimes > 0) {
				--retryTimes

				/** @type { success: boolaen; allChunksCompleted: boolean } */
				let uploadResult
				try {
					uploadResult = await (await fetch(`${uriPrefix}${file.name}`, {
						method: 'PUT',
						headers: {
							'Resumable-Upload-Chunk-Index': chunkIndex,
						},
						body: data,
					})).json()
				} catch (_) {
					continue
				}

				if (!uploadResult.success) {
					continue
				} else {
					completedChunks += 1
					uploadProgress = completedChunks / totalChunks

					if (uploadResult.allChunksCompleted) {
						if (stopProgressBarAnimation != null) {
							stopProgressBarAnimation()
							stopProgressBarAnimation = null
						}
						hideUploadProgress()
						alert('Upload complete!')
						window.location.reload()
					}

					break
				}
			}
		})()
	})
}

function showUploadProgress() {
	uploadProgressContainerElement.style.display = 'block'
}

function hideUploadProgress() {
	uploadProgressContainerElement.removeAttribute('style')
	uploadProgressBarElement.removeAttribute('style')
	uploadProgressTextElement.innerText = '0%'
}

/**
 * Starts the progress bar animation.
 * @param {Object} options - The options for the progress bar animation.
 * @param {() => number} options.getProgress - The function for getting the current progress (0 - 1).
 * @returns {() => void} - The function to stop the progress bar animation.
 */
function startProgressBarAnimation(options) {
	let isRunning = true

	function run() {
		const progress = options.getProgress()
		uploadProgressTextElement.innerText = `${Math.floor(progress * 100)}%`
		uploadProgressBarElement.style.transform = `scaleX(${progress})`

		if (isRunning) {
			requestAnimationFrame(run)
		}
	}

	requestAnimationFrame(run)

	return function stopProgressBarAnimation() {
		isRunning = false
	}
}

function prettyFileSize(fileSizeInBytes) {
	if (fileSizeInBytes == null) {
		return 'unknown size'
	}
	if (fileSizeInBytes < 1024) {
		return fileSizeInBytes + ' B'
	}
	if (fileSizeInBytes < 1024 * 1024) {
		return (fileSizeInBytes / 1024).toFixed(2) + ' KiB'
	}
	if (fileSizeInBytes < 1024 * 1024 * 1024) {
		return (fileSizeInBytes / (1024 * 1024)).toFixed(2) + ' MiB'
	}
	if (fileSizeInBytes < 1024 * 1024 * 1024 * 1024) {
		return (fileSizeInBytes / (1024 * 1024 * 1024)).toFixed(2) + ' GiB'
	}
	if (fileSizeInBytes < 1024 * 1024 * 1024 * 1024 * 1024) {
		return (fileSizeInBytes / (1024 * 1024 * 1024 * 1024)).toFixed(2) + ' TiB'
	}
}

/**
 * @description If you have lots of promises to be resolved,
 * but you don't want to resolve them all at once,
 * use this pool to keep the number of concurrent promises under control.
 * @param {number} concurrency - The maximum number of concurrent promises.
 * @param {() => (Promise<void> | null)} spawnPromise - The function to spawn a promise. Returns `null` means there's no more promise to spawn.
 */
async function makePromisePool(concurrency, spawnPromise) {
	/** @type Promise<{ promise: Promise }>[] */
	const pool = []

	/**
	 * @description Spawn a wrapped promise that the resolved value is an object contains the wrapped promise itself.
	 * @param {Promise<void>} innerPromise - The inner promise.
	 * @returns {Promise<{ promise: Promise }>} - The wrapped promise.
	 */
	function spwanWrappedPromise(innerPromise) {
		const { promise, resolve, reject } = Promise.withResolvers();
		innerPromise.then(() => {
			resolve({promise})
		}).catch(reject)
		return promise
	}

	while (pool.length < concurrency) {
		const promise = spawnPromise()
		if (promise == null) {
			break
		}
		pool.push(spwanWrappedPromise(promise))
	}

	while (pool.length !== 0) {
		const resovledPromise = await Promise.race(pool)
		const resovledIndex = pool.findIndex(p => p === resovledPromise.promise)
		pool.splice(resovledIndex, 1)

		const newPromise = spawnPromise()
		if (newPromise != null) {
			pool.push(spwanWrappedPromise(newPromise))
		}
	}
}
