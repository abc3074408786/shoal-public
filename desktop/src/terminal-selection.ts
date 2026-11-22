/** Remove only the shared space margin; preserve line endings and relative indentation. */
export function stripTerminalSelectionGutter(selection:string):string {
  const content=selection.match(/[^\r\n]+/g)?.filter(line=>/[^ ]/.test(line))??[];
  if(!content.length)return selection;
  let margin=Infinity;
  for(const line of content)margin=Math.min(margin,line.search(/[^ ]/));
  if(!margin)return selection;
  return selection.replace(new RegExp(`^ {1,${margin}}`,'gm'),'');
}
