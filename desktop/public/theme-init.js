try{document.documentElement.dataset.theme=JSON.parse(localStorage.getItem('velella:ui:theme'))==='light'?'light':'dark'}catch{document.documentElement.dataset.theme='dark'}
